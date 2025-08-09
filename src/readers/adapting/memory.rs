/// Pressure pushing down on me pressing down on you
/// no man ask for under pressure
/// that burns a building down
/// splits a family in two
/// puts people on streets
use crate::readers::ChunkStrategy;

#[derive(Debug, Clone, Copy)]
pub enum MemoryPressure {
    None,     // < 50% usage
    Low,      // 50-70%
    Medium,   // 70-85%
    High,     // 85-95%
    Critical, // > 95%
}

pub fn detect_memory_pressure() -> MemoryPressure {
    // Use memory_stats to get current usage
    // Compare to available memory
    // Return appropriate level
    todo!();
}

pub fn available_memory_mb() -> f64 {
    todo!();
}

pub fn process_memory_mb() -> f64 {
    todo!();
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStrategy {
    // TODO
}

impl ChunkStrategy for MemoryStrategy {
    fn initial_chunk_size(&self) -> usize {
        todo!()
    }

    fn adapt_size(&mut self, metrics: &super::ChunkMetrics, current_size: usize) -> usize {
        todo!()
    }

    fn should_adapt(&self, metrics: &super::ChunkMetrics) -> bool {
        // metrics.should_adapt(interval, chunks_processed)
        todo!()
    }
}
