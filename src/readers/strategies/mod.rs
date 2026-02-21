//! Chunk sizing strategy for ChunkedFileReader.
//!
//! Uses a static strategy that picks an optimal chunk size based on
//! file size and sticks with it. This is the right approach for an
//! I/O tool where JSON parsing dominates runtime.

mod fixed;

pub use fixed::*;

use crate::config::ConfigOpts;

impl StaticStrategy {
    /// Create from CLI options and file size hint
    pub fn from_config(_config: &ConfigOpts, size_hint: Option<u64>) -> Self {
        if let Some(size) = size_hint {
            StaticStrategy::optimal_for_file(size)
        } else {
            StaticStrategy::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_from_config() {
        let config = ConfigOpts::default();
        let strategy = StaticStrategy::from_config(&config, Some(50_000_000));
        assert!(strategy.chunk_size > 0);
    }
}
