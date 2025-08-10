//! An adaptive chunked reader, which uses its current strategy
//! to decide how to read incoming data. Look, don't complain that I'm
//! overengineering a hobby project. The whole point of a hobby project
//! is learning through overengineering. And why are you reading this
//! comment, anyway?

use super::detect_memory_pressure;
use super::metrics::{ChunkMetrics, MovingAverage};
use super::{ChunkStrategy, ChunkedFileReader, MemoryPressure, MetricsCollector, Trend};
use crate::constants::INITIAL_CHUNK_SIZE;

/// Here's the reader type that can adapt.
#[derive(Debug)]
pub struct AdaptiveChunkReader<T>
where
    T: ChunkStrategy,
{
    reader: ChunkedFileReader,
    controller: AdaptationController<T>,
}

#[derive(Debug, Clone)]
pub struct AdaptationController<T>
where
    T: ChunkStrategy,
{
    strategy: T,
    metrics: MetricsCollector,
    config: AdaptationConfig,
}

impl<T> AdaptationController<T>
where
    T: ChunkStrategy,
{
    pub fn should_adapt(&self) -> bool {
        self.metrics.ready_to_adapt(self.config.interval)
    }

    pub fn calculate_new_size(&mut self, current: usize) -> usize {
        let metrics = self.metrics.snapshot();
        self.strategy.adapt_size(&metrics, current)
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveStrategy {
    config: AdaptationConfig,
    metrics_history: MovingAverage<10>,
}

impl Default for AdaptiveStrategy {
    fn default() -> Self {
        let config = AdaptationConfig::default();
        Self {
            config,
            metrics_history: MovingAverage::new(),
        }
    }
}

// We adapt based on performance and memory data as we go
impl ChunkStrategy for AdaptiveStrategy {
    fn initial_chunk_size(&self) -> usize {
        self.config.initial_chunk_size
    }

    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize {
        let pressure = detect_memory_pressure(Some(self.config.memory_threshold_mb));

        // PRESSURE RELEASE VALVE - immediate drop to minimum
        if matches!(pressure, MemoryPressure::Critical) {
            eprintln!("Memory critical! Dropping to minimum chunk size");
            return self.config.min_chunk_size;
        }

        // High pressure - gradual reduction
        if matches!(pressure, MemoryPressure::High) {
            return (current_size as f64 * 0.5) as usize; // Aggressive shrink
        }

        let newsize = match pressure {
            MemoryPressure::Critical => self.config.min_chunk_size,
            MemoryPressure::High => (current_size as f64 * self.config.shrink_factor) as usize,
            _ => {
                let moving_ave = metrics.speed_moving();
                let perf = moving_ave.trend(self.config.speed_increase_threshold);
                match perf {
                    Trend::Improving => (current_size as f64 * self.config.growth_factor) as usize,
                    Trend::Degrading => (current_size as f64 * self.config.shrink_factor) as usize,
                    Trend::Stable => current_size,
                    Trend::Unknown => current_size,
                }
            }
        };
        let clamped = newsize.clamp(self.config.min_chunk_size, self.config.max_chunk_size);
        self.metrics_history.push(clamped as f64);
        clamped
    }

    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool {
        metrics.should_adapt(self.config.interval)
    }
}

#[derive(Debug, Clone)]
pub struct AdaptationConfig {
    // Size boundaries (in bytes)
    pub min_chunk_size: usize,     // Don't go below this (e.g., 4KB)
    pub max_chunk_size: usize,     // Don't exceed this (e.g., 1MB)
    pub initial_chunk_size: usize, // Starting point (e.g., 32KB)

    // When to grow/shrink
    pub speed_increase_threshold: f64,  // If speed improves by X%, grow chunks
    pub _speed_decrease_threshold: f64, // If speed drops by X%, shrink chunks
    pub memory_threshold_mb: usize,     // Shrink if we use more than this

    // How much to grow/shrink
    pub growth_factor: f64, // Multiply size by this (e.g., 1.5)
    pub shrink_factor: f64, // Multiply size by this (e.g., 0.67)

    // How often to check
    pub interval: usize, // Check every N chunks (e.g., 5)
}

impl Default for AdaptationConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 4 * 1024,    // 4K bytes
            max_chunk_size: 1024 * 1024, // 1MB
            initial_chunk_size: INITIAL_CHUNK_SIZE,
            speed_increase_threshold: Default::default(),
            _speed_decrease_threshold: Default::default(),
            memory_threshold_mb: Default::default(),
            growth_factor: 1.5,
            shrink_factor: 0.67,
            interval: 5,
        }
    }
}
