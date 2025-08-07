//! Reading files by adaptive chunk sizes to adapt to memory pressure
//! and file size. We want to scale down if we're on a puny container
//! on some Raspberry Pi in the cloud, and scale up if we're on monstro
//! the memory-stuffed real-hardware luxury box. We also want to cope with
//! what we're being asked to juggle.

mod adaptive;
mod metrics;

pub use adaptive::AdaptiveProcessor;

// - [ ] Create `ChunkMetrics` struct: processing_speed, memory_usage,
//   io_wait_time, lines_per_chunk
// - [ ] Design `AdaptationConfig` with thresholds and limits
// - [ ] Define strategy types: `Static`, `Adaptive`, `MemoryConstrained`

/// This is how a strategy advertises itself to an adaptable reader.
pub trait ChunkStrategy {
    /// What chunk size should we start with?
    fn initial_chunk_size();
    /// What should we use now?
    fn adapt_size();
    /// Should we change?
    fn should_adapt() -> bool;
}

/// Taking Tiger Mountain by
pub enum Strategy {
    Static,
    Adaptive,
    MemoryConstrained,
}

/// When do we shift gears anyway? What's the RPM threshold?
pub struct AdaptationConfig {
    // TODO
}
