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
use tabwriter::TabWriter;
use term_grid::{Filling, Grid, GridOptions};

#[derive(Debug, Clone, Parser)]
#[clap(name="bistre", version, styles = v3_styles(), max_term_width = 100)]
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
    #[arg(short, long, global = true)]
    timestamps: bool,
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long, global = true)]
    follow: bool,
    /// If prefixed with -, the number of lines from the end to start reading
    /// from. If prefixed with +, the number of lines from the start.
    /// Only makes sense if you're tailing a file.
    #[arg(default_value = "0")]
    offset: String,
    /// Pretty-print the named file; defaults to printing stdin if not provided.
    #[arg(default_value = "")]
    tail: String,
}

/// I like my clap help styled the old way.
fn v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// A log line, somewhat flexibly formatted. There are two fields that we demand be present: a log level and a message. We allow several possible names for common log fields. The rest of the fields are mentioned only so we can print them in a controlled order. We do not demand that they be present.
#[derive(Debug, Clone, Deserialize)]
struct Message {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    timestamp: Option<jiff::Timestamp>,
    /// The string message part of the log line.
    #[serde(alias = "msg")]
    message: String,
    /// Log level for this line.
    #[serde(alias = "lvl")]
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

impl Message {
    pub fn write<T>(&self, tabby: &mut TabWriter<T>, show_time: bool) -> anyhow::Result<()>
    where
        T: Write,
    {
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

        // Now we print.
        let _results: Vec<Result<(), std::io::Error>> = cells
            .iter()
            .enumerate()
            .map(|(i, xs)| {
                if i % 4 == 0 {
                    write!(tabby, " \t")?;
                }
                if i % 3 == 0 {
                    writeln!(tabby)?;
                }
                write!(tabby, "{xs}\t")?;
                Ok(())
            })
            .collect();
        Ok(())
    }
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

#[derive(Debug, Clone, Default)]
struct Soot {
    tailing: bool,
    show_time: bool,
}

static CONFIG: OnceLock<Soot> = OnceLock::new();

fn cat_stdin() -> anyhow::Result<()> {
    let show_time = CONFIG.get().is_some_and(|v| v.show_time);

    let mut line = String::new();
    let mut inlock = io::stdin().lock();
    let mut outlock = io::stdout().lock();
    // let mut tabby = TabWriter::new(outlock).ansi(true).minwidth(10);

    while inlock.read_line(&mut line)? != 0 {
        match serde_json::from_str::<Message>(line.as_str()) {
            Ok(message) => {
                writeln!(outlock, "{message}");
                //message.write(&mut tabby, show_time)?;
                //tabby.flush()?;
            }
            Err(_) => {
                writeln!(outlock, "{line}");
                //tabby.write_all(line.as_bytes())?;
                //tabby.flush()?;
            }
        }
        line.clear();
    }
    if !CONFIG.get().is_some_and(|v| v.tailing) {
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

fn tail(fpath: PathBuf, offset: i64) -> anyhow::Result<()> {
    use std::io::Seek;

    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }

    let show_time = CONFIG.get().is_some_and(|v| v.show_time);
    let tailing = CONFIG.get().is_some_and(|v| v.tailing);

    let outlock = io::stdout().lock();
    let mut tabby = TabWriter::new(outlock).ansi(true).minwidth(10);

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
        let _ignored = by_lines.by_ref().take(offset as usize);
    };

    // Now at last we get to start printing. What a fuss. We need to call flush
    // periodically so tabwriter writes, but we don't want to call it too often or
    // it won't align usefully.
    const FLUSH_INTERVAL: u8 = 40;

    let mut count: u8 = 0;
    for maybe_line in by_lines {
        let Ok(line) = maybe_line else {
            break;
        };
        match serde_json::from_str::<Message>(&line) {
            Ok(message) => {
                message.write(&mut tabby, show_time)?;
                if count >= FLUSH_INTERVAL {
                    tabby.flush()?;
                    count = 0;
                }
                count += 1;
            }
            Err(_) => {
                writeln!(tabby, "{line}")?;
                count = 0;
                tabby.flush()?;
            }
        }
    }
    tabby.flush()?;

    if !CONFIG.get().is_some_and(|v| v.tailing) {
        return Ok(());
    }

    // TODO if tailing, need to hang out and read more

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let config = Soot {
        tailing: args.follow,
        show_time: args.timestamps,
    };
    CONFIG
        .set(config)
        .expect("Quite improbably failed to set a OnceLock on process start.");

    // A bit fiddly but: if we see args.offset = "0" and args.tail = empty,
    // we do stdin and stop at EOF. If we see args.offset = some string and
    // args.tail = empty, we tail the given file from the end. Otherwise,
    // we do the tail the named file from the offset thing. We do not yet
    // try to be fancy and tail multiple files. If we will ever do such a thing.

    if args.offset == "0" && args.tail.is_empty() {
        cat_stdin()
    } else if args.tail.is_empty() {
        let fpath = PathBuf::from(args.offset);
        tail(fpath, 0)
    } else {
        let fpath = PathBuf::from(args.tail);
        let offset: i64 = args.offset.parse::<i64>()?;
        tail(fpath, offset)
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
