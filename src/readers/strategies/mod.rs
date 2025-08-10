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

use crate::config::ConfigOpts;
use crate::metrics::{ChunkMetrics, is_memory_constrained};

/// This is how a strategy advertises itself to an adaptable reader.
pub trait IsStrategy: std::fmt::Debug {
    /// What chunk size should we start with?
    fn initial_chunk_size(&self) -> usize;
    /// Should we change?
    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool;
    /// What should we use now?
    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize;
}

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
        if crate::metrics::is_memory_constrained() {
            Strategy::Conservative(ConservativeStrategy::default())
        } else {
            Strategy::Adaptive(AdaptiveStrategy::default())
        }
    }

    /// Pick the best strategy based on context
    pub fn auto_select() -> Self {
        if is_memory_constrained() {
            Strategy::Conservative(ConservativeStrategy::default())
        } else {
            Strategy::Adaptive(AdaptiveStrategy::default())
        }
    }

    /// Create from CLI options
    pub fn from_config(config: &ConfigOpts) -> Self {
        // Explicit strategy from CLI
        if let Some(strategy) = &config.strategy {
            return strategy.clone();
        }

        // Conservative mode in debug
        if config.conservative {
            return Strategy::Conservative(ConservativeStrategy::default());
        }

        // Adaptive is off
        if !config.adaptive {
            return Strategy::Static(StaticStrategy::default());
        }

        // Auto-select based on environment
        Self::auto_select()
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
    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool {
        match self {
            Strategy::Static(s) => s.should_adapt(metrics),
            Strategy::Adaptive(s) => s.should_adapt(metrics),
            Strategy::Conservative(s) => s.should_adapt(metrics),
        }
    }

    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize {
        match self {
            Strategy::Static(s) => s.adapt_size(metrics, current_size),
            Strategy::Adaptive(s) => s.adapt_size(metrics, current_size),
            Strategy::Conservative(s) => s.adapt_size(metrics, current_size),
        }
    }

    fn initial_chunk_size(&self) -> usize {
        match self {
            Strategy::Static(s) => s.initial_chunk_size(),
            Strategy::Adaptive(s) => s.initial_chunk_size(),
            Strategy::Conservative(s) => s.initial_chunk_size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_selection() {
        // Test with adaptive enabled
        let config = ConfigOpts {
            adaptive: true,
            strategy: None,
            conservative: false,
            ..Default::default()
        };

        let strategy = Strategy::from_config(&config);
        assert!(matches!(strategy, Strategy::Adaptive(_)));
    }
}
