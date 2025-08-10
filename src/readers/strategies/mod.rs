//! We climbed and we climbed oh how we climbed
//! My, how we climbed
//! Over the stars to top Tiger Mountain
//! Forcing the lines through the snow

mod adaptive;
mod conservative;
mod fixed;

pub use adaptive::*;
pub use conservative::*;
pub use fixed::*;

/// This is how a strategy advertises itself to an adaptable reader.
pub trait IsStrategy {
    /// What chunk size should we start with?
    fn initial_chunk_size(&self) -> usize;
    /// What should we use now?
    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize;
    /// Should we change?
    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool;
}

use crate::constants::INITIAL_CHUNK_SIZE;
use crate::readers::ChunkMetrics;

/// Taking Tiger Mountain by
#[derive(Debug, Clone)]
pub enum Strategy {
    /// Use a fixed chunk size; gains from changing would be marginal at best
    Static(StaticStrategy),
    /// Choose a dynamic sizing based on metrics
    Adaptive(AdaptiveStrategy),
    /// Prioritize memory limits over performance
    Conservative(ConservativeStrategy),
}

impl Strategy {
    pub fn pick_strategy() -> Strategy {
        // Always adaptive unless in constrained environment
        if super::is_memory_constrained() {
            Strategy::Conservative(ConservativeStrategy::default())
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
            "memory" => Self::Conservative(ConservativeStrategy::default()),
            _ => Self::default(),
        }
    }
}

impl IsStrategy for Strategy {
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
