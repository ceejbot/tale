//! Pretty-print newline-delimited json (ndjson) logs.
//! No more, no less.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bytes::{Buf, BytesMut};
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;

mod loglines;
use loglines::*;

/// How long we wait before flushing data to stdout when tailing.
static TAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Parser)]
#[clap(name="tale", version, styles = v3_styles(), max_term_width = 100)]
/// A tool for pretty-printing json logs or any ndjson content that has
/// a message, a level, and a timestamp.
///
/// The timestamp field may be named `time`, `ts`, or `timestamp`. The message
/// field may be named `message` or `msg`. The tool has some opinions about
/// ordering for fields commonly found in server log structures, but will print
/// out every field that shows up in the log line, using the color theme you
/// have set in your terminal.
struct Args {
    /// Show timestamps, which are hidden by default.
    #[arg(short, long)]
    timestamps: bool,
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long)]
    follow: bool,
    /// Arguments: [offset] [file] where offset can be -N, +N, or N
    #[arg(allow_hyphen_values = true)]
    args: Vec<String>,
}

/// I like my clap help styled the old way.
fn v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[derive(Debug, Clone, Default)]
struct ConfigOpts {
    tailing: bool,
    show_time: bool,
}

static CONFIG: OnceLock<ConfigOpts> = OnceLock::new();

/// Process a single line of input (JSON or plain text) and write to output.
#[inline]
fn process_line(line: &str, buffer: &mut BytesMut, outlock: &mut io::StdoutLock<'_>) -> anyhow::Result<()> {
    match serde_json::from_str::<Printable<'_>>(line) {
        Ok(message) => {
            message.write(buffer);
            outlock.write_all(buffer.chunk())?;
            outlock.write_all(&[0x0a; 1])?; // blank line
            buffer.clear();
        }
        Err(_) => {
            outlock.write_all(line.as_bytes())?;
            outlock.write_all(b"\n")?;
        }
    }
    Ok(())
}

/// Strip trailing newline(s) to match BufReader::lines() behavior
#[inline]
fn strip_line_ending(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            // Windows line endings are not handled well by Rust's line
            // iterators, but we might as well try.
            line.pop();
        }
    }
}

fn handle_stdin(tail: bool) -> anyhow::Result<()> {
    use std::time::{Duration, Instant};

    // Pre-allocate based on typical log line length (~200-300 chars)
    let mut line = String::with_capacity(512);
    let mut inlock = io::stdin().lock();
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);
    const FLUSH_INTERVAL: u8 = 40;
    let mut count: u8 = 0;

    // Process initial input until EOF
    while inlock.read_line(&mut line)? != 0 {
        strip_line_ending(&mut line);
        process_line(&line, &mut buffer, &mut outlock)?;
        count += 1;
        if count >= FLUSH_INTERVAL {
            outlock.flush()?;
            count = 0;
        }
        line.clear();
    }
    outlock.flush()?;

    if !tail {
        return Ok(());
    }

    // We flush at intervals when tailing, because we might wait a
    // long time for more data.
    let mut last_flush = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(100));

        match inlock.read_line(&mut line)? {
            0 => {
                // EOF - no new data available, continue polling
                continue;
            }
            _ => {
                strip_line_ending(&mut line);
                // New data available - process it.
                process_line(&line, &mut buffer, &mut outlock)?;
                if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                    outlock.flush()?;
                    last_flush = Instant::now();
                }

                line.clear();
            }
        }
    }
}

/// Find the byte offset from the beginning of the file for the start of the
/// line to begin our pretty-printing. This is the seek backwards version. It is
/// made entirely of edge cases.
fn seek_backwards(file: &mut File, lines_from_end: u64) -> anyhow::Result<u64> {
    let file_size = file.seek(io::SeekFrom::End(0))?;
    if file_size == 0 {
        return Ok(0);
    }

    const BUFFER_SIZE: usize = 8192;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut lines_found = 0u64;

    // First check if the file ends with a newline
    file.seek(io::SeekFrom::End(-1))?;
    let mut last_byte = [0u8; 1];
    file.read_exact(&mut last_byte)?;
    let ends_with_newline = last_byte[0] == b'\n';

    // To get the last N lines, we need to find the right number of newlines
    // For a file that doesn't end with newline: last line is after the last newline
    // For a file that ends with newline: last line is between the last two newlines
    let target_newlines = if ends_with_newline {
        lines_from_end
    } else {
        lines_from_end - 1
    };

    let mut pos = file_size;

    loop {
        // Calculate how much to read in this chunk
        let chunk_size = std::cmp::min(BUFFER_SIZE as u64, pos) as usize;
        if chunk_size == 0 {
            // We've reached the beginning of the file
            return Ok(0);
        }

        // Read a chonk. Chunk. Whatever.
        pos -= chunk_size as u64;
        file.seek(io::SeekFrom::Start(pos))?;
        file.read_exact(&mut buffer[..chunk_size])?;

        // Count newlines in reverse order
        for (i, &byte) in buffer[..chunk_size].iter().enumerate().rev() {
            if byte == b'\n' {
                lines_found += 1;
                if lines_found > target_newlines {
                    // Found enough lines, return position after this newline
                    return Ok(pos + i as u64 + 1);
                }
            }
        }

        // We hit the beginning: not enough lines. We start at the very
        // beginning, a very good place to start.
        if pos == 0 {
            return Ok(0);
        }
    }
}

fn handle_file(fpath: PathBuf, offset: i64) -> anyhow::Result<()> {
    use std::io::Seek;

    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }

    let tailing = CONFIG.get().is_some_and(|v| v.tailing);

    let mut file = File::open(&fpath)?;

    // Set our position in the file.
    if offset > 0 {
        // Positive offset: skip N lines from the beginning, so somewhat
        // pointlessly set ourselves at the start.
        file.seek(io::SeekFrom::Start(0))?;
    } else if offset < 0 {
        // Negative offset: start N lines from the end
        let start = seek_backwards(&mut file, (-offset) as u64)?;
        file.seek(io::SeekFrom::Start(start))?;
    } else if tailing {
        // Zero offset: start from the end (no lines to show unless tailing)
        file.seek(io::SeekFrom::End(0))?;
    };

    let mut reader = BufReader::new(file);

    // If we've got a positive offset, we still need to skip our N lines
    if offset > 0 {
        let consume_me = (&mut reader).lines().take(offset as usize);
        // We then must consume them. this feels v inefficient but I do not know.
        let _count = consume_me.count();
    };

    // Now at last we get to start printing. What a fuss.
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);
    let mut line = String::with_capacity(1024);
    const FLUSH_INTERVAL: u8 = 40;

    let mut count: u8 = 0;
    while reader.read_line(&mut line)? != 0 {
        strip_line_ending(&mut line);
        process_line(&line, &mut buffer, &mut outlock)?;
        count += 1;
        if count >= FLUSH_INTERVAL {
            outlock.flush()?;
            count = 0;
        }
        line.clear();
    }
    outlock.flush()?;

    if !tailing {
        return Ok(());
    }

    // Now we tell a tale of tailing.
    let mut last_flush = Instant::now();

    // Get the file back from the reader
    let mut file = reader.into_inner();
    let mut file_position = file.stream_position()?;

    loop {
        std::thread::sleep(Duration::from_millis(100));

        // Check if file has grown
        let current_size = file.seek(io::SeekFrom::End(0))?;
        if current_size > file_position {
            // Hide and seek, trains and sewing machines.
            file.seek(io::SeekFrom::Start(file_position))?;
            let mut tail_reader = BufReader::new(&file);

            match tail_reader.read_line(&mut line)? {
                0 => {
                    // EOF - no new data available, continue polling
                    continue;
                }
                _ => {
                    strip_line_ending(&mut line);
                    // New data available - process it.
                    process_line(&line, &mut buffer, &mut outlock)?;
                    if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                        outlock.flush()?;
                        last_flush = Instant::now();
                    }

                    line.clear();
                    buffer.clear();
                }
            }

            // Note where we finished reading so we can figure out if we get more.
            file_position = file.stream_position()?;
        }
    }
}

fn parse_tail_args(args: Vec<String>) -> anyhow::Result<(Option<i64>, Option<PathBuf>)> {
    match args.len() {
        0 => Ok((None, None)), // stdin
        1 => {
            let first = &args[0];
            if (first.starts_with('-') || first.starts_with('+'))
                && first.len() > 1
                && let Ok(offset) = first.parse::<i64>()
            {
                // It's a numeric offset like "-4" or "+4"
                Ok((Some(offset), None)) // stdin with offset
            } else {
                // It's a filename
                Ok((None, Some(PathBuf::from(first))))
            }
        }
        2 => {
            let (offset_str, file_str) = (&args[0], &args[1]);
            let offset = offset_str.parse::<i64>()?;
            let file_path = PathBuf::from(file_str);
            Ok((Some(offset), Some(file_path)))
        }
        _ => Err(anyhow!("Too many arguments")),
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let config = ConfigOpts {
        tailing: args.follow,
        show_time: args.timestamps,
    };
    CONFIG
        .set(config)
        .expect("Quite improbably failed to set config OnceLock on process start.");

    let (offset, file_path) = parse_tail_args(args.args)?;

    match (offset, file_path) {
        (None, None) => {
            // No arguments: read from stdin
            handle_stdin(args.follow)
        }
        (Some(_offset), None) => {
            // Offset only: read from stdin with offset (not really useful but supported)
            handle_stdin(args.follow)
        }
        (None, Some(file_path)) => {
            // File only: read entire file
            handle_file(file_path, 0)
        }
        (Some(offset), Some(file_path)) => {
            // Both offset and file: read file from offset
            handle_file(file_path, offset)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn seeking_backwards() {
        use std::io::{Read, Seek, Write};

        use tempfile::NamedTempFile;

        // Create a temporary file with known content
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let content = "line1\nline2\nline3\nline4\nline5\n";
        temp_file
            .write_all(content.as_bytes())
            .expect("Failed to write to temp file");

        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        // Test getting last 2 lines (should start after "line3\n")
        let pos = seek_backwards(&mut file, 2).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line4\nline5\n");

        // Test getting last line (should start after "line4\n")
        let pos = seek_backwards(&mut file, 1).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line5\n");

        // Test getting more lines than exist (should start from beginning)
        let pos = seek_backwards(&mut file, 10).expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn seeking_in_empty() {
        use tempfile::NamedTempFile;
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        let pos = seek_backwards(&mut file, 5).expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn layout_one() {
        let logline = r##"{
            "timestamp": "2025-08-01T10:45:03Z",
            "level": "CRITICAL",
            "message": "Database query failed",
            "query": "SELECT * FROM users WHERE id = ?",
            "error_code": "ER_NO_SUCH_TABLE",
            "elapsed": "250ms"
        }"##;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
        let stringy = parsed.to_string();
        let lines: Vec<&str> = stringy.split('\n').collect();
        let length = lines.len();
        assert_eq!(length, 5);
    }
}
