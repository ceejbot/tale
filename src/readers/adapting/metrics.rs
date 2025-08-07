/// Collecting system metrics and maintaining a sliding window.
/// This will be fun.

// use sysinfo;

/// We want to track how we're doing so we know when to shift down into second
/// gear and ride the clutch or when we're winding up into high rpms and need to
/// upshift.
pub struct ChunkMetrics {
    /// TODO guessing at how we'll measure this; bytes/sec?
    processing_speed: f64,
    /// memory usage in bytes
    memory_bytes: usize,
    /// my favorite OS stat to look at
    io_wait_time: usize,
    lines_per_chunk: u64,
}
