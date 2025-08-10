//! Reading files by adaptive chunk sizes to adapt to memory pressure
//! and file size. We want to scale down if we're on a puny container
//! on some Raspberry Pi in the cloud, and scale up if we're on monstro
//! the memory-stuffed real-hardware luxury box. We also want to cope with
//! what we're being asked to juggle.

use super::*;
use crate::constants::INITIAL_CHUNK_SIZE;
use crate::metrics::*;
use crate::readers::ChunkedFileReader;

/// Here's the reader type that can adapt.
#[derive(Debug)]
pub struct AdaptiveChunkReader<T>
where
    T: IsStrategy,
{
    reader: ChunkedFileReader,
    controller: AdaptationController<T>,
}

#[derive(Debug, Clone)]
pub struct AdaptationController<T>
where
    T: IsStrategy,
{
    strategy: T,
    collector: MetricsCollector,
    config: AdaptationConfig,
    adaptations_made: usize,
}

impl<T> AdaptationController<T>
where
    T: IsStrategy,
{
    pub fn should_adapt(&self) -> bool {
        self.collector.ready_to_adapt(self.config.interval)
    }

    pub fn calculate_new_size(&mut self, current: usize) -> usize {
        self.adaptations_made += 1;
        let metrics = self.collector.snapshot();
        self.strategy.adapt_size(metrics, current)
    }

    /// For debug output.
    pub fn adaptation_stats(&self) -> AdaptationStats {
        AdaptationStats {
            chunks_processed: self.collector.chunks_seen(),
            avg_chunk_size: self.collector.chunk_sizes_avg(),
            avg_speed_mbps: self.collector.processing_speed_mbps(),
            adaptations_made: self.adaptations_made,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdaptationStats {
    chunks_processed: usize,
    avg_chunk_size: f64,
    avg_speed_mbps: f64,
    adaptations_made: usize,
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
impl IsStrategy for AdaptiveStrategy {
    fn initial_chunk_size(&self) -> usize {
        self.config.initial_chunk_size
    }

    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize {
        let pressure = detect_memory_pressure(Some(self.config.memory_threshold_mb));

        #[cfg(debug_assertions)]
        eprintln!(
            "Adaptation: chunks={}, speed={:.2}MB/s, memory={}MB, pressure={:?}",
            metrics.chunks_seen,
            metrics.processing_speed_mbps(),
            metrics.memory_bytes / (1024 * 1024),
            pressure
        );

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
            min_chunk_size: 4 * 1024,
            max_chunk_size: 1024 * 1024,
            initial_chunk_size: INITIAL_CHUNK_SIZE,
            speed_increase_threshold: 5.0,   // 5% improvement to grow
            _speed_decrease_threshold: -5.0, // 5% degradation to shrink
            memory_threshold_mb: 200,        // 200MB default limit
            growth_factor: 1.5,
            shrink_factor: 0.67,
            interval: 5,
        }
    }
}
