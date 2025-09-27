//! This reader only shrinks down to a minimum size.
//! We'll choose it when we're in very constrained environments.

use super::IsStrategy;
use crate::config::*;
use crate::metrics::{ChunkMetrics, MemoryPressure, detect_memory_pressure};

#[derive(Debug, Clone, Default)]
pub struct ConservativeStrategy {
    config: ConservativeConfig,
}

impl IsStrategy for ConservativeStrategy {
    fn initial_chunk_size(&self) -> usize {
        self.config.calculate_limit_bytes() / self.config.num_files
    }

    /// Given the current chunk size and current metrics, make a decision about
    /// what chunk size we should use next.
    fn adapt_size(&mut self, _metrics: &super::ChunkMetrics, current_size: usize) -> usize {
        let max = config().max_memory;
        match detect_memory_pressure(max) {
            MemoryPressure::Unknown => current_size,
            MemoryPressure::None => current_size,
            MemoryPressure::Low => {
                // Slight reduction to be conservative
                std::cmp::max(current_size * 9 / 10, Self::MIN_CHUNK_SIZE)
            }
            MemoryPressure::Medium => {
                // Moderate reduction
                std::cmp::max(current_size * 3 / 4, Self::MIN_CHUNK_SIZE)
            }
            MemoryPressure::High => {
                // Significant reduction
                std::cmp::max(current_size / 2, Self::MIN_CHUNK_SIZE)
            }
            MemoryPressure::Critical => {
                // Drop to minimum to avoid OOM
                Self::MIN_CHUNK_SIZE
            }
        }
    }

    /// Should we do the work of making an adaptation decision right now or just
    /// keep sailing?
    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool {
        metrics.should_adapt(self.config.interval)
    }
}

impl ConservativeStrategy {
    /// Minimum chunk size we're willing to use - 4KB should be enough to read a
    /// few lines efficiently
    const MIN_CHUNK_SIZE: usize = 4 * 1024;
}

#[derive(Debug, Clone, Default)]
pub struct ConservativeConfig {
    // User-specified limit
    pub max_total_bytes: Option<usize>,
    // Count of files being processed
    pub num_files: usize,
    pub interval: usize, // Check every N chunks
}

impl ConservativeConfig {
    const MEMORY_CEILING_BYTES: usize = 200 * 1024 * 1024;
    const PER_FILE_BYTES: usize = 10 * 1024 * 1024;

    /// Responds with a limit in megabytes.
    pub fn calculate_limit_bytes(&self) -> usize {
        if let Some(user_limit) = self.max_total_bytes {
            return user_limit;
        }

        // If we have the space, we give each file a fairly roomy 10MB. This is
        // probably more than we need, and we can tighten after measuring.
        let total_for_files = ConservativeConfig::PER_FILE_BYTES * self.num_files;

        // System limit: 10% of RAM or 200MB, whichever is smaller
        let system_total = crate::metrics::get_system_ram_bytes();
        let system_limit = std::cmp::min(system_total / 10, ConservativeConfig::MEMORY_CEILING_BYTES);

        std::cmp::min(total_for_files, system_limit)
    }
}
