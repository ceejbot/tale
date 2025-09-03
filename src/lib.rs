//! Tale - A high-performance log pretty-printer for ndjson files
//!
//! This library provides utilities for reading, parsing, and formatting
//! newline-delimited JSON log files with memory-efficient processing
//! and adaptive chunking strategies.

use std::io::{self, Write};

use bytes::{Buf, BytesMut};
use logpatterns::*;
use miette::{IntoDiagnostic, Result};

pub mod config;
pub mod defaults;
pub mod errors;
pub mod json_profiler;
pub mod logpatterns;
pub mod memory_budget;
pub mod metrics;
pub mod multiplexed;
pub mod readers;

#[cfg(test)]
mod tests;

// Re-export commonly used types for convenience
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
pub use errors::TaleError;
pub use memory_budget::{MemoryBudget, MemoryPressure};
pub use readers::strategies::{IsStrategy, Strategy};
pub use readers::{ChunkedFileReader, FileProcessor};

#[derive(Debug, Clone, Parser, Default)]
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
pub struct Args {
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long)]
    pub follow: bool,
    /// Follow the file, also checking to see if has been renamed or has an new
    /// inode number. If the file does not exist yet, wait and display it
    /// from the beginning if and when it is created.
    #[arg(short = 'F', long)]
    pub sticky: bool,
    /// Start tailing the input offset by ±N blocks.
    #[arg(short, long, group = "units")]
    pub blocks: Option<i64>,
    /// Start tailing the input offset by ±N bytes; e.g., to skip garbage.
    #[arg(short = 'c', long, group = "units")]
    pub bytes: Option<i64>,
    /// Start tailing the input offset by ±N lines.
    #[arg(short = 'n', long, group = "units")]
    pub offset: Option<i64>,
    /// When following more than one file, show a header with the file name
    /// along with every line from that file.
    #[arg(short, long)]
    pub verbose: bool,
    /// Do not ever show file name headers when following more than one file.
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    // these options are unique to `tale`
    /// Show timestamps, which are hidden by default.
    #[arg(short, long)]
    pub timestamps: bool,
    /// Batch window size for multi-file tailing (in milliseconds).
    #[arg(long, default_value = "250")]
    pub window: u64,
    /// Generate completions for the given shell.
    #[arg(long)]
    pub completions: Option<clap_complete::Shell>,

    // TODO These are options we should consider making dev-only.
    /// Force use of chunked file processing for better memory efficiency on
    /// large files.
    #[arg(long)]
    pub chunked: bool,
    /// Disable chunked file processing and always use streaming (might use more
    /// memory).
    #[arg(long, conflicts_with = "chunked")]
    pub no_chunked: bool,
    /// Disable adaptive chunking
    #[arg(short, long)]
    pub adaptive: bool,
    /// Set a limit on how much memory can be used in file buffers
    #[arg(short, long)]
    pub max_memory: Option<usize>,
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub conservative: bool,
    #[cfg(debug_assertions)]
    /// Choose a specific chunk strategy for testing
    #[arg(short = 's', long)]
    pub chunk_strategy: Option<Strategy>,
    /// Print JSON parsing profile report after processing (debug builds only)
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub profile_json: bool,

    /// (offset) [file ...] where offset can be +N, -N, or N.
    #[arg(allow_hyphen_values = true)]
    pub args: Vec<String>,
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
pub fn process_line(line: &str, buffer: &mut BytesMut, outlock: &mut io::StdoutLock<'_>) -> Result<()> {
    match serde_json::from_str::<Printable<'_>>(line) {
        Ok(message) => {
            // Profile which variant was parsed (debug builds only for minimal overhead)
            #[cfg(debug_assertions)]
            json_profiler::record_variant(&message);

            message.write(buffer);
            outlock.write_all(buffer.chunk()).into_diagnostic()?;
            outlock.write_all(&[0x0a; 1]).into_diagnostic()?; // blank line
            buffer.clear();
        }
        Err(_) => {
            // Profile parse failures (debug builds only)
            #[cfg(debug_assertions)]
            json_profiler::record_parse_error();

            outlock.write_all(line.as_bytes()).into_diagnostic()?;
            outlock.write_all(b"\n").into_diagnostic()?;
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

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn offset_unit_args() {
        use crate::Args;
        use crate::config::{ConfigOpts, OffsetUnit};

        // Test bytes offset detection - test the config struct directly
        let args = Args {
            bytes: Some(100),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert!(matches!(config.offset_unit, OffsetUnit::Bytes));
        assert_eq!(config.offset, 100);

        // Test blocks offset detection
        let args = Args {
            blocks: Some(2),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert!(matches!(config.offset_unit, OffsetUnit::Blocks));
        assert_eq!(config.offset, 2);

        // Test lines offset detection (default)
        let args = Args {
            offset: Some(5),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert!(matches!(config.offset_unit, OffsetUnit::Lines));
        assert_eq!(config.offset, 5);
    }

    #[test]
    fn can_run_cli_and_emit_help() {
        let output = std::process::Command::new("cargo")
            .args(["run", "--", "--help"])
            .output()
            .expect("failed to execute");

        assert!(output.status.success());
        // Verify output is correct
    }
}
