//! Pretty-print newline-delimited json (ndjson) logs.
//! No more, no less.

use std::fmt::Display;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::anyhow;
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;
use term_grid::{Filling, Grid, GridOptions};

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

/// A log line, somewhat flexibly formatted. There are two fields that we demand
/// be present: a log level and a message. We allow several possible names for
/// common log fields. The rest of the fields are mentioned only so we can print
/// them in a controlled order. We do not demand that they be present.
#[derive(Debug, Clone, Deserialize)]
struct Message {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    timestamp: Option<jiff::Timestamp>,
    /// The string message part of the log line.
    #[serde(alias = "msg")]
    message: String,
    /// Log level for this line.
    #[serde(alias = "lvl", alias = "severity")]
    level: String,
    /// A request id
    #[serde(alias = "requestId")]
    request_id: Option<String>,
    /// Remote host.
    #[serde(alias = "hostname", alias = "remote_host")]
    host: Option<String>,
    /// http method
    method: Option<String>,
    /// a url
    url: Option<String>,
    /// http response status code
    #[serde(alias = "statusCode", alias = "code")]
    status: Option<String>,
    /// elapsed time
    #[serde(alias = "elapsed_ms", alias = "elapsed_time")]
    elapsed: Option<String>,
    /// size of the written response
    #[serde(alias = "sent_bytes", alias = "length", alias = "written")]
    bytes: Option<String>,
    /// The unpredictable parts of the log line we want to handle generally.
    #[serde(flatten)]
    rest: serde_json::Value,
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
        let mut cells: Vec<String> = Vec::new();

        if show_time && let Some(ref v) = self.timestamp {
            cells.push(format!("{}", v.strftime("%F-%T").blue()));
        }

        cells.push(format!("{}", self.level.bold().bright_blue()));
        cells.push(format!("{}", self.message.bold()));

        if let Some(ref v) = self.request_id {
            cells.push(format!("reqid: {}", v.bright_yellow()));
        }
        if let Some(ref v) = self.host {
            cells.push(format!("host: {}", v.green()));
        }
        if let Some(ref v) = self.method {
            cells.push(format!("{}", v.blue()));
        }
        if let Some(ref v) = self.url {
            cells.push(format!("{}", v.blue()));
        }
        if let Some(ref v) = self.status {
            cells.push(format!("{}", v.purple()));
        }
        // todo humanize ? unsure the units here might vary, so maybe not.
        if let Some(ref v) = self.elapsed {
            cells.push(format!("elapsed: {}", v.bright_purple()));
        }
        // todo humanize ? same comment as above.
        if let Some(ref v) = self.bytes {
            cells.push(format!("written: {}", v.magenta()));
        }
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(format!("{}: {}", key.dimmed(), value.cyan()));
                });
            }
            _ => {
                let the_rest = self.rest.as_str().unwrap_or_default();
                cells.push(the_rest.to_string());
            }
        }

        let grid = Grid::new(
            cells,
            GridOptions {
                filling: Filling::Spaces(2),
                direction: term_grid::Direction::LeftToRight,
                width: 120,
            },
        );
        write!(f, "{grid}")
    }
}

/*

CRITICAL module::thing::foobl > message here with rest of width
LEVEL    MODULE or TIME       > message line wrapped if no line endings in it; as-is if \n present
8char    20 char              > termwidth - 32 char wide
blank    timing / bytes       > STATUS VERB URL if present
         bytes                > key: value pairs to fill space, wrapping, left aligned at this column

 */
fn _layout(_cells: Vec<String>) -> Vec<String> {
    let size = terminal_size::terminal_size();
    let _width = if let Some((terminal_size::Width(w), _)) = size {
        w
    } else {
        80
    };

    todo!()
}

#[derive(Debug, Clone, Default)]
struct Tally {
    tailing: bool,
    show_time: bool,
}

static CONFIG: OnceLock<Tally> = OnceLock::new();

fn handle_stdin(tail: bool) -> anyhow::Result<()> {
    let mut line = String::new();
    let mut inlock = io::stdin().lock();
    let mut outlock = io::stdout().lock();

    while inlock.read_line(&mut line)? != 0 {
        match serde_json::from_str::<Message>(line.as_str()) {
            Ok(message) => {
                writeln!(outlock, "{message}")?;
            }
            Err(_) => {
                writeln!(outlock, "{line}")?;
            }
        }
        line.clear();
    }
    if !tail {
        return Ok(());
    }

    // TODO if tailing, need to hang out and read more

    Ok(())
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

        // Seek to the beginning of this chunk
        pos -= chunk_size as u64;
        file.seek(io::SeekFrom::Start(pos))?;

        // Read the chunk
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

    let reader = BufReader::new(file);
    let mut by_lines = reader.lines();

    // If we've got a positive offset, we still need to skip our N lines
    if offset > 0 {
        println!("we are skipping {offset} lines");
        let consume_me = by_lines.by_ref().take(offset as usize);
        // we then must consume them. this feels v inefficient but I do not know.
        let _count = consume_me.count();
    };

    // Now at last we get to start printing. What a fuss.
    let mut outlock = io::stdout().lock();
    const FLUSH_INTERVAL: u8 = 40;

    let mut count: u8 = 0;
    for maybe_line in by_lines {
        let Ok(line) = maybe_line else {
            break;
        };
        match serde_json::from_str::<Message>(&line) {
            Ok(message) => {
                write!(outlock, "{message}")?;
                if count >= FLUSH_INTERVAL {
                    outlock.flush()?;
                    count = 0;
                }
                count += 1;
            }
            Err(_) => {
                writeln!(outlock, "{line}")?;
                count = 0;
                outlock.flush()?;
            }
        }
    }
    outlock.flush()?;

    if !CONFIG.get().is_some_and(|v| v.tailing) {
        return Ok(());
    }

    // TODO if tailing, need to hang out and read more

    Ok(())
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

    let config = Tally {
        tailing: args.follow,
        show_time: args.timestamps,
    };
    CONFIG
        .set(config)
        .expect("Quite improbably failed to set a OnceLock on process start.");

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
    fn logline_deser() {
        let logline = r#"{
            "timestamp": "2025-07-30T17:41-07:00",
            "level":"INFO",
            "message": "I'm not crazy you're the ones who are crazy"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.message, "I'm not crazy you're the ones who are crazy");
        let logline = r#"{"timestamp": "2025-07-30T17:41-07:00",
            "message":"I'm not crazy you're the ones who are crazy",
            "level":"WARN",
            "request_id":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));
        let logline = r#"{"ts": "2025-07-30T17:41-07:00",
            "msg":"I'm not crazy you're the ones who are crazy",
            "lvl":"CRITICAL",
            "requestId":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));
    }

    #[test]
    fn not_loglines() {
        let logline = r#"Sometimes, I try to do things / And it just doesn't work out the way I want it to"#;
        let error = serde_json::from_str::<Message>(logline);
        assert!(error.is_err());
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
}
