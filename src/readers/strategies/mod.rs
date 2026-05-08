//! Chunk sizing strategy for ChunkedFileReader.
//!
//! Uses a static strategy that picks an optimal chunk size based on
//! file size and sticks with it. This is the right approach for an
//! I/O tool where JSON parsing dominates runtime.

mod fixed;

pub use fixed::*;
