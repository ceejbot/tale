//! Pretty-print newline-delimited json (ndjson) logs, with a tail-compatible
//! set of options and behaviors.

mod batch;
mod config;
mod errors;
mod file_state;
mod logpatterns;
mod multiplexed;
mod readers;
mod watcher;

use std::io::{self, Write};
use std::time::Duration;

use bytes::{Buf, BytesMut};
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use config::{ConfigOpts, config};
use logpatterns::*;
use miette::Result as MietteResult;

use crate::config::InputMode;
use crate::errors::TaleError;

#[derive(Debug, Clone, Parser)]
#[clap(name="tale", version, styles = v3_styles(), max_term_width = 100)]
/// A tail-compatible tool for pretty-printing ndjson files, especially logs.
///
/// Tale displays the colorfully-formatted contents of FILE, by default stdin,
/// to stdout. It highlights the fields likely to appear in log lines for
/// servers, such as level or severity, the log message, timestamps, and so
/// on. It also displays every field that shows up in the log line, using
/// the color theme you have set in your terminal.
///
/// Lines that are invalid json are printed intact, without formatting.
///
/// Tale can also follow and display more than one file at a time, with
/// header decoration options like `tail`'s.
struct Args {
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long)]
    follow: bool,
    /// Follow the file, also checking to see if has been renamed or has an new
    /// inode number. If the file does not exist yet, wait and display it
    /// from the beginning if and when it is created.
    #[arg(short = 'F', long)]
    sticky: bool,
    /// Start tailing the input offset by ±N blocks.
    #[arg(short, long, group = "units")]
    blocks: Option<i64>,
    /// Start tailing the input offset by ±N bytes; e.g., to skip garbage.
    #[arg(short = 'c', long, group = "units")]
    bytes: Option<i64>,
    /// Start tailing the input offset by ±N lines.
    #[arg(short = 'n', long, group = "units")]
    offset: Option<i64>,
    /// When following more than one file, show a header with the file name
    /// along with every line from that file. Not yet implemented.
    #[arg(short, long)]
    verbose: bool,
    /// Do not ever show file name headers when following more than one file.
    /// Not yet implemented.
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,

    // these options are unique to `tale`
    /// Show timestamps, which are hidden by default.
    #[arg(short, long)]
    timestamps: bool,
    /// Batch window size for multi-file tailing (in milliseconds).
    #[arg(long, default_value = "250")]
    window: u64,
    /// Force use of chunked file processing for better memory efficiency on
    /// large files.
    #[arg(long)]
    chunked: bool,
    /// Disable chunked file processing and always use streaming (might use more
    /// memory).
    #[arg(long, conflicts_with = "chunked")]
    no_chunked: bool,

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

pub mod constants {
    use super::Duration;

    /// How long we wait before flushing data to stdout when tailing.
    pub const TAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

    /// Flush stdout when we've written at least this many lines.
    pub const FLUSH_LINE_COUNT: u16 = 40;

    /// The traditional unix block size in bytes.
    pub const BLOCK_SIZE: u64 = 512;

    /// Buffer size for reading from stdin/files.
    pub const READ_BUFFER_SIZE: usize = 8192;

    /// Default capacity for line strings.
    pub const LINE_CAPACITY: usize = 512;

    /// Default capacity for output byte buffers.
    pub const OUTPUT_BUFFER_CAPACITY: usize = 1024;

    /// Memory limit for line buffering in negative line offset mode.
    pub const MEMORY_LIMIT_BYTES: usize = 10 * 1024 * 1024; // 10MB
}

// TODO: move the two inlines somewhere sensible.

/// Process a single line of input (JSON or plain text) and write to output.
#[inline]
pub fn process_line(line: &str, buffer: &mut BytesMut, outlock: &mut io::StdoutLock<'_>) -> Result<(), TaleError> {
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

/// Strip trailing newline(s) from the string input, handling Windows line
/// endings as well.
#[inline]
pub fn strip_line_ending(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            // Windows line endings are not handled well by Rust's line
            // iterators, but we might as well try.
            line.pop();
        }
    }
}

/// Parse our options and do the thing.
#[tokio::main]
async fn main() -> MietteResult<()> {
    let args = Args::parse();

    let config = ConfigOpts::new(&args);
    config::set(config).expect("Quite improbably failed to set config OnceLock on process start.");

    let mode = config::mode();
    let result = match mode {
        InputMode::Stdin => readers::handle_stdin(),
        InputMode::SingleFile { path } => readers::handle_file(&path),
        InputMode::MultiFile { paths } => {
            if args.follow || args.sticky {
                // Multi-file tailing mode
                multiplexed::handle_tailing(paths).await
            } else {
                // Multi-file static mode (read all files once)
                multiplexed::handle_static(paths)
            }
        }
    };

    // Convert TaleError to miette Report for display
    result.map_err(miette::Report::from)
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
    fn offset_unit_args() {
        use crate::config::OffsetUnit;
        use crate::{Args, ConfigOpts};

        // Test bytes offset detection - test the config struct directly
        let args = Args {
            timestamps: false,
            follow: false,
            sticky: false,
            blocks: None,
            bytes: Some(100),
            offset: None,
            verbose: false,
            quiet: false,
            window: 250,
            chunked: false,
            no_chunked: false,
            args: vec!["test.log".to_string()],
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Bytes));
        assert_eq!(config.offset, 100);

        // Test blocks offset detection
        let args = Args {
            timestamps: false,
            follow: false,
            sticky: false,
            blocks: Some(2),
            bytes: None,
            offset: None,
            verbose: false,
            quiet: false,
            window: 250,
            chunked: false,
            no_chunked: false,
            args: vec!["test.log".to_string()],
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Blocks));
        assert_eq!(config.offset, 2);

        // Test lines offset detection (default)
        let args = Args {
            timestamps: false,
            follow: false,
            sticky: false,
            blocks: None,
            bytes: None,
            offset: Some(5),
            verbose: false,
            quiet: false,
            window: 250,
            chunked: false,
            no_chunked: false,
            args: vec!["test.log".to_string()],
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Lines));
        assert_eq!(config.offset, 5);
    }
}
