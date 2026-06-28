//! Tale - A high-performance log pretty-printer for ndjson files
//!
//! This library provides utilities for reading, parsing, and formatting
//! newline-delimited JSON log files with memory-efficient processing
//! and chunked file processing.
//!
//! ## Library surface
//!
//! This crate is, first and foremost, the `tale` binary. The `pub` items here
//! exist to serve that binary and the `benches/` — both separate crate targets,
//! so they can only reach `pub` items. They are **not** a stability-guaranteed
//! public API. Items re-exported solely for the benchmarks are marked
//! `#[doc(hidden)]`; `TaleError` and the [`FileProcessor`] trait are the only
//! pieces shaped as a reusable surface.

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

// The error type and the core reader trait are the reusable surface.
pub use errors::TaleError;
// Re-exported only so `benches/` (a separate crate target) can reach them.
// Still `pub` so the benchmarks compile; hidden from the published docs.
#[doc(hidden)]
pub use memory_budget::{MemoryBudget, MemoryPressure};
#[doc(hidden)]
pub use readers::ChunkedFileReader;
pub use readers::FileProcessor;
#[doc(hidden)]
pub use readers::strategies::StaticStrategy;

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
