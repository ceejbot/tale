//! We climbed and we climbed oh how we climbed
//! My, how we climbed
//! Over the stars to top Tiger Mountain
//! Forcing the lines through the snow

// use super::AdaptationConfig;
use super::*;
use crate::constants::INITIAL_CHUNK_SIZE;

/// Taking Tiger Mountain by
#[derive(Debug, Clone)]
pub enum Strategy {
    /// Use a fixed chunk size; gains from changing would be marginal at best
    Static(StaticStrategy),
    /// Choose a dynamic sizing based on metrics
    Adaptive(AdaptiveStrategy),
    /// Prioritize memory limits over performance
    Conservative(MemoryStrategy),
}

impl Strategy {
    pub fn pick_strategy() -> Strategy {
        // Always adaptive unless in constrained environment
        if is_memory_constrained() {
            Strategy::Conservative(MemoryStrategy::default())
        } else {
            Strategy::Adaptive(AdaptiveStrategy::default())
        }
    }
}

impl Default for Strategy {
    fn default() -> Self {
        Strategy::Static(StaticStrategy::default())
    }
}

impl From<&str> for Strategy {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "static" => Self::default(),
            "adaptive" => Self::Adaptive(AdaptiveStrategy::default()),
            "memory" => Self::Conservative(MemoryStrategy::default()),
            _ => Self::default(),
        }
    }
}

impl ChunkStrategy for Strategy {
    fn initial_chunk_size(&self) -> usize {
        match self {
            Strategy::Static(v) => v.initial_chunk_size(),
            Strategy::Adaptive(v) => v.initial_chunk_size(),
            Strategy::Conservative(_) => INITIAL_CHUNK_SIZE, // TODO
        }
    }

    fn adapt_size(&mut self, metrics: &super::metrics::ChunkMetrics, current_size: usize) -> usize {
        match self {
            Strategy::Static(_) => current_size,
            Strategy::Adaptive(v) => v.adapt_size(metrics, current_size),
            Strategy::Conservative(v) => v.adapt_size(metrics, current_size),
        }
    }

    fn should_adapt(&self, metrics: &super::metrics::ChunkMetrics) -> bool {
        match self {
            Strategy::Static(_) => false,
            Strategy::Adaptive(v) => v.should_adapt(metrics),
            Strategy::Conservative(v) => v.should_adapt(metrics),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StaticStrategy {
    chunk_size: usize,
}

impl StaticStrategy {
    pub fn conservative() -> Self {
        Self {
            chunk_size: 4 * 1024, // 4K bytes
        }
    }
}

impl Default for StaticStrategy {
    fn default() -> Self {
        Self {
            chunk_size: INITIAL_CHUNK_SIZE,
        }
    }
}

impl ChunkStrategy for StaticStrategy {
    /// the fixed chunk size we should use; defaults to INITIAL_CHUNK_SIZE
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
