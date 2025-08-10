//! This strategy picks an optimal chunk size at the start of reading
//! and then doesn't vary it but chugs right on through. This is fine for
//! single-file reading.

use super::IsStrategy;
use crate::constants::READ_BUFFER_SIZE;
use crate::metrics::ChunkMetrics;

#[derive(Debug, Clone)]
pub struct StaticStrategy {
    pub chunk_size: usize,
    pub config: ChunkConfig,
}

impl StaticStrategy {
    pub fn conservative() -> Self {
        let config = ChunkConfig {
            overlap_size: 1024,
            low_memory_mode: true,
        };
        Self { 
            chunk_size: 4 * 1024 * 1024,
            config 
        }
    }

    pub fn optimal_for_file(file_size: u64) -> Self {
        let config = ChunkConfig::optimal(file_size);
        let chunk_size = optimal_chunk_size(file_size, None);
        Self { chunk_size, config }
    }

    pub fn with_config(config: ChunkConfig) -> Self {
        Self { 
            chunk_size: READ_BUFFER_SIZE, // Default chunk size
            config 
        }
    }
}

impl Default for StaticStrategy {
    fn default() -> Self {
        Self {
            chunk_size: READ_BUFFER_SIZE,
            config: ChunkConfig::default(),
        }
    }
}

impl IsStrategy for StaticStrategy {
    /// the fixed chunk size we should use
    fn initial_chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Don't change.
    fn adapt_size(&mut self, _metrics: &ChunkMetrics, current_size: usize) -> usize {
        current_size // Never change
    }

    fn should_adapt(&self, _metrics: &ChunkMetrics) -> bool {
        false // Never adapt
    }
}

/// Configuration for FileChunk processing (chunk_size moved to Strategy)
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Maximum overlap between chunks to handle line boundaries
    pub overlap_size: usize,
    /// Whether to use memory-constrained processing
    pub low_memory_mode: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            overlap_size: 1024, // 1KB overlap for line boundaries
            low_memory_mode: false,
        }
    }
}

impl ChunkConfig {
    pub fn optimal(_file_size: u64) -> Self {
        Self {
            overlap_size: 1024,
            low_memory_mode: false,
        }
    }
}

/// Determine optimal chunk size based on file characteristics
pub fn optimal_chunk_size(file_size: u64, available_memory: Option<usize>) -> usize {
    let default_memory = 10 * 1024 * 1024; // 10MB default
    let memory_limit = available_memory.unwrap_or(default_memory);

    match file_size {
        // Small files: use small chunks to minimize overhead
        0..=1_000_000 => 8_192, // 8KB

        // Medium files: balance memory and I/O efficiency
        1_000_001..=100_000_000 => 32_768, // 32KB

        // Large files: use large chunks but respect memory limits
        _ => std::cmp::min(262_144, memory_limit / 10), // 256KB max, or 10% of available memory
    }
}
