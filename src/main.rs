//! Pretty-print newline-delimited json (ndjson) logs.
//! No more, no less.

mod batch;
mod columns;
mod config;
mod file_state;
mod loglines;
mod watcher;

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bytes::{Buf, BytesMut};
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use config::{ConfigOpts, config};
use loglines::*;

use crate::config::InputMode;

/// Extract timestamp from a JSON line without full parsing to Printable
/// This does single parsing for timestamp extraction only
/// How long we wait before flushing data to stdout when tailing.
static TAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Parser)]
#[clap(name="tale", version, styles = v3_styles(), max_term_width = 100)]
/// A tail-compatible tool for pretty-printing ndjson files, especially logs.
///
/// It displays the colorfully-formatted contents of FILE, by default stdin,
/// to stdout. Tale highlights the fields likely to appear in log lines for
/// servers, such as level or severity, the log message, timestamps, and so
/// on. It also displays every field that shows up in the log line,  using
/// the color theme you have set in your terminal.
///
/// Lines that are invalid json are printed intact, without formatting.
///
/// `tail` can also follow and display more than one file at a time, with
/// header decoration options like `tail`'s.
struct Args {
    /// Show timestamps, which are hidden by default.
    #[arg(short, long)]
    timestamps: bool,
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long)]
    follow: bool,
    /// Follow the file, also checking to see if has been renamed or has an new
    /// inode number. If the file does not exist yet, wait and display it
    /// from the beginning if and when it is created.
    #[arg(short = 'F', long)]
    sticky: bool,
    /// Start tailing offset by N blocks.  Not yet respected.
    #[arg(short, long, group = "units")]
    blocks: Option<i64>,
    /// Start tailing offset by N bytes; e.g., to skip garbage.  Not yet
    /// respected.
    #[arg(short = 'c', long, group = "units")]
    bytes: Option<i64>,
    /// Start tailing offset by N lines. Not yet respected.
    #[arg(short = 'n', long, group = "units")]
    offset: Option<i64>,
    /// When following more than one file, show a header with the file name
    /// along with every line from that file.  Not yet respected.
    #[arg(short, long)]
    verbose: bool,
    /// Do not ever show file name headers when following more than one file.
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,

    /// Batch window size for multi-file tailing (in milliseconds).
    #[arg(long, default_value = "250")]
    window: u64,
    /// Arguments: (offset) [file ...] where offset can be +N, -N, or N.
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

fn handle_file(fpath: &PathBuf, offset: i64) -> anyhow::Result<()> {
    use std::io::Seek;

    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }

    let tailing = config::tailing();

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let config = ConfigOpts::new(&args);
    config::set(config).expect("Quite improbably failed to set config OnceLock on process start.");
    let mode = config::mode();
    let offset = config::offset();

    match mode {
        InputMode::Stdin { .. } => {
            // Handle stdin with optional offset (TODO implement offset for stdin)
            handle_stdin(args.follow)
        }
        InputMode::SingleFile { path } => {
            // Handle single file with offset
            handle_file(path, offset)
        }
        InputMode::MultiFile { paths } => {
            if args.follow {
                // Multi-file tailing mode
                handle_multi_file_tailing(paths.to_vec()).await
            } else {
                // Multi-file static mode (read all files once)
                handle_multi_file_static(paths.to_vec())
            }
        }
    }
}

/// Handle multi-file static mode (read all files once, no following)
fn handle_multi_file_static(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    use crate::file_state::FileStateManager;

    let mut file_manager = FileStateManager::new();

    // Add all files to the manager
    for path in &paths {
        file_manager.add_file(path)?;
    }

    // Read all lines from all files (for static reading)
    let all_lines = file_manager.read_all_lines()?;

    // Collect all lines, parse and convert to SourcedLine immediately
    let mut all_sourced_lines: Vec<SourcedLine<'_>> = Vec::new();

    all_lines.iter().for_each(|(file_path, lines)| {
        lines.iter().enumerate().for_each(|(line_num, line_content)| {
            let parsed: Printable<'_> = {
                match serde_json::from_str::<Printable<'_>>(&line_content) {
                    Ok(printable) => printable,
                    Err(_) => {
                        // If parsing as Printable fails, it's either invalid JSON or doesn't match any
                        // variant Use the Text fallback in either case
                        Printable::Text(line_content.clone())
                    }
                }
            };
            let sourced = SourcedLine::new(parsed, file_path.clone(), line_num);
            all_sourced_lines.push(sourced);
        });
    });
    // Sort using the stable multi-file sorting function
    all_sourced_lines.sort_by_key(|line| line.sort_key());

    // Output the sorted lines using existing process_line function
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);

    for wrapped in all_sourced_lines {
        wrapped.write(&mut buffer);
        outlock.write_all(buffer.chunk())?;
        outlock.write_all(&[0x0a; 1])?; // blank line
        buffer.clear();
    }

    outlock.flush()?;
    Ok(())
}

/// Handle multi-file tailing mode (watch for changes and follow)
async fn handle_multi_file_tailing(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    use crate::batch::{BatchConfig, BatchedLine, create_processor_with_config};
    use crate::watcher::{WatchEvent, create_watcher};

    // Create the file watcher
    let mut watcher = create_watcher();

    // Add files to watch
    watcher.add_files(paths).await?;

    // Create batch processor with configuration from CLI
    let batch_config = BatchConfig {
        batch_window: Duration::from_millis(config::batch_window_ms()),
        max_batch_size: 1000,
        max_buffer_memory: 10 * 1024 * 1024,
    };
    let mut batch_processor = create_processor_with_config(batch_config);

    // Start the batch processor
    let (line_sender, mut batch_receiver) = batch_processor.start().await?;

    // Start watching files
    let mut watch_events = watcher.watch().await?;

    // Set up output
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);

    // Main coordination loop
    loop {
        tokio::select! {
            // Handle file system events
            watch_event = watch_events.recv() => {
                match watch_event {
                    Some(WatchEvent::FileModified(path)) => {
                        // File was modified, read new lines
                        if let Some(state) = watcher.file_manager_mut().get_state_mut(&path) {
                            if let Ok(_changed) = state.refresh() {
                                if let Ok(new_lines) = state.read_new_lines() {
                                    // Send lines to batch processor
                                    for (line_num, line) in new_lines.into_iter().enumerate() {
                                        let batched_line = BatchedLine::new(
                                            line,
                                            path.clone(),
                                            line_num as u64
                                        );
                                        if let Err(_) = line_sender.send(batched_line) {
                                            return Err(anyhow!("Batch processor stopped"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(WatchEvent::Error(err)) => {
                        eprintln!("Watch error: {}", err);
                    }
                    Some(_) => {
                        // Other events (create, delete) - could handle these in future
                    }
                    None => {
                        // Watcher stopped
                        break;
                    }
                }
            }

            // Handle sorted batches from batch processor
            batch = batch_receiver.recv() => {
                match batch {
                    Some(sorted_lines) => {
                        // Output the sorted batch
                        for batched_line in sorted_lines {
                            process_line(&batched_line.content, &mut buffer, &mut outlock)?;
                        }
                        outlock.flush()?;
                    }
                    None => {
                        // Batch processor stopped
                        break;
                    }
                }
            }
        }
    }

    Ok(())
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
        config::set(ConfigOpts::default()).expect("tests should successfully set config");

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

        assert_eq!(length, 4);
    }
}
