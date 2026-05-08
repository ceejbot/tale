//! Tale - A high-performance log pretty-printer for ndjson files
//!
//! This library provides utilities for reading, parsing, and formatting
//! newline-delimited JSON log files with memory-efficient processing
//! and chunked file processing.

use std::io::{self, Write};

use bytes::{Buf, BytesMut};
use logpatterns::*;
use miette::{IntoDiagnostic, Result};

pub mod config;
pub(crate) mod defaults;
pub(crate) mod errors;
#[cfg(debug_assertions)]
pub mod json_profiler;
pub(crate) mod logpatterns;
pub(crate) mod memory_budget;
pub mod metrics;
pub mod multiplexed;
pub mod readers;

#[cfg(test)]
mod tests;

// Re-export commonly used types for convenience
pub use errors::TaleError;
pub use memory_budget::{MemoryBudget, MemoryPressure};
pub use readers::strategies::StaticStrategy;
pub use readers::{ChunkedFileReader, FileProcessor};

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
            outlock.write_all(b"\n").into_diagnostic()?; // blank line between log entries
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
