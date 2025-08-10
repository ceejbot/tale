//! Named constants, rather than magic ones. All times & durations should
//! be in milliseconds. All sizes should be in bytes.

use super::Duration;

/// How long we wait before flushing data to stdout when tailing.
pub const TAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Flush stdout when we've written at least this many lines.
pub const FLUSH_LINE_COUNT: u16 = 40;

/// Default capacity for line strings.
pub const LINE_CAPACITY: usize = 512;

/// The traditional unix block size in bytes.
pub const BLOCK_SIZE: u64 = 512;

/// Buffer size for reading from stdin/files.
pub const READ_BUFFER_SIZE: usize = 8192;

/// Default capacity for output byte buffers.
pub const OUTPUT_BUFFER_CAPACITY: usize = 1024;

/// Memory limit for line buffering in negative line offset mode.
pub const MEMORY_LIMIT_BYTES: usize = 10 * 1024 * 1024; // 10MB

/// The initial chunk size to use for adaptive chunked readers.
pub const INITIAL_CHUNK_SIZE: usize = 32 * 1024; // 32K bytes maybe???
