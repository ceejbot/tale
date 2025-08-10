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
        todo!()
    }

    /// Given the current chunk size and current metrics, make a decision about
    /// what chunk size we should use next.
    fn adapt_size(&mut self, _metrics: &super::ChunkMetrics, current_size: usize) -> usize {
        let max = config().max_memory;
        match detect_memory_pressure(max) {
            MemoryPressure::None => current_size,
            MemoryPressure::Low => todo!(),
            MemoryPressure::Medium => todo!(),
            MemoryPressure::High => todo!(),
            MemoryPressure::Critical => todo!(),
        }
    }

    /// Should we do the work of making an adaptation decision right now or just
    /// keep sailing?
    fn should_adapt(&self, _metrics: &ChunkMetrics) -> bool {
        // metrics.should_adapt(interval, chunks_processed)
        todo!()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConservativeConfig {
    // User-specified limit
    pub max_total_mb: Option<usize>,
    // Count of files being processed
    pub num_files: usize,
}

impl ConservativeConfig {
    const MEMORY_CEILING_MB: usize = 200;
    const PER_FILE_MB: usize = 10;

    /// Responds with a limit in megabytes.
    pub fn calculate_limit_mb(&self) -> usize {
        if let Some(user_limit) = self.max_total_mb {
            return user_limit;
        }

        // If we have the space, we give each file a fairly
        // roomy 10MB. This is probably more than we need, and we
        // can tighten after measuring.
        let total_for_files = ConservativeConfig::PER_FILE_MB * self.num_files;

        // System limit: 10% of RAM or 200MB, whichever is smaller
        let system_limit = if let Some(total_ram_mb) = crate::metrics::get_system_ram_mb() {
            std::cmp::min(total_ram_mb / 10, ConservativeConfig::MEMORY_CEILING_MB)
        } else {
            ConservativeConfig::MEMORY_CEILING_MB // Conservative fallback
        };

        std::cmp::min(total_for_files, system_limit)
    }
}
