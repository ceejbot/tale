//! Reading files by adaptive chunk sizes to adapt to memory pressure
//! and file size. We want to scale down if we're on a puny container
//! on some Raspberry Pi in the cloud, and scale up if we're on monstro
//! the memory-stuffed real-hardware luxury box. We also want to cope with
//! what we're being asked to juggle.

mod adaptive;
mod memory;
mod metrics;
mod strategies;

use std::fmt::Debug;

use adaptive::*;
use memory::*;
pub use metrics::*;
pub use strategies::*;

// - [ ] Create `ChunkMetrics` struct: processing_speed, memory_usage,
//   io_wait_time, lines_per_chunk
// - [ ] Design `AdaptationConfig` with thresholds and limits
// - [ ] Define strategy types: `Static`, `Adaptive`, `MemoryConstrained`

/// This is how a strategy advertises itself to an adaptable reader.
pub trait ChunkStrategy {
    /// What chunk size should we start with?
    fn initial_chunk_size(&self) -> usize;
    /// What should we use now?
    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize;
    /// Should we change?
    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool;
}

/// When do we shift gears anyway? What's the RPM threshold?
#[derive(Debug, Clone)]
pub struct AdaptationConfig {
    // Size boundaries
    pub chunk_size_min: usize,     // 4KB minimum
    pub chunk_size_ax: usize,      // 1MB maximum
    pub chunk_size_initial: usize, // 32KB default

    // Adaptation thresholds
    pub speed_increase_threshold: f64,  // 1.2x speed → increase size
    pub speed_decrease_threshold: f64,  // 0.8x speed → decrease size
    pub memory_pressure_threshold: f64, // 80% memory usage

    // Adjustment factors
    pub growth_factor: f64,         // 1.5x growth
    pub shrink_factor: f64,         // 0.67x shrink
    pub adaptation_interval: usize, // Adapt every 5 chunks
}
