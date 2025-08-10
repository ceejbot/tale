//! Reading files by adaptive chunk sizes to adapt to memory pressure
//! and file size. We want to scale down if we're on a puny container
//! on some Raspberry Pi in the cloud, and scale up if we're on monstro
//! the memory-stuffed real-hardware luxury box. We also want to cope with
//! what we're being asked to juggle.

use std::time::{Duration, Instant};

use super::*;
use crate::constants::INITIAL_CHUNK_SIZE;
use crate::metrics::*;

// AdaptiveChunkReader and AdaptationController removed - functionality moved to Strategy enum

#[derive(Debug, Clone)]
struct MemoryCache {
    last_check: Instant,
    last_pressure: MemoryPressure,
}

impl MemoryCache {
    fn get_pressure(&mut self) -> MemoryPressure {
        // Only check every 1 second
        if self.last_check.elapsed() > Duration::from_secs(1) {
            self.last_pressure = detect_memory_pressure(None);
            self.last_check = Instant::now();
        }
        self.last_pressure
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveStrategy {
    pub config: AdaptationConfig,
    metrics_history: MovingAverage<10>,
    memory: MemoryCache,
}

impl Default for AdaptiveStrategy {
    fn default() -> Self {
        let config = AdaptationConfig::default();
        Self {
            config,
            metrics_history: MovingAverage::new(),
            memory: MemoryCache {
                last_check: Instant::now(),
                last_pressure: MemoryPressure::Unknown,
            },
        }
    }
}

impl AdaptiveStrategy {
    pub fn optimal_for_file(file_size: u64) -> Self {
        let mut config = AdaptationConfig::default();
        // Start at the same size static would use
        config.initial_chunk_size = optimal_chunk_size(file_size, None);
        Self {
            config,
            ..Default::default()
        }
    }
}

// We adapt based on performance and memory data as we go
impl IsStrategy for AdaptiveStrategy {
    fn initial_chunk_size(&self) -> usize {
        self.config.initial_chunk_size
    }

    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size_bytes: usize) -> usize {
        let pressure = self.memory.get_pressure();
        // let pressure = detect_memory_pressure(Some(self.config.memory_threshold_mb));

        /*
        #[cfg(debug_assertions)]
        {
            let speed = metrics.processing_speed_mbps();
            let trend = metrics.speed_moving().trend(self.config.speed_increase_threshold);
            eprintln!(
                "Adapt: chunk#{}, size={}, speed={:.2}MB/s, trend={:?}, pressure={:?}",
                metrics.chunks_seen, current_size, speed, trend, pressure
            );
        }
        */

        // PRESSURE RELEASE VALVE - immediate drop to minimum
        if matches!(pressure, MemoryPressure::Critical) {
            eprintln!("Memory critical! Dropping to minimum chunk size");
            return self.config.min_chunk_size;
        }

        // High pressure - gradual reduction
        if matches!(pressure, MemoryPressure::High) {
            return (current_size_bytes as f64 * 0.5) as usize; // Aggressive shrink
        }

        let newsize = match pressure {
            MemoryPressure::Critical => self.config.min_chunk_size,
            MemoryPressure::High => (current_size_bytes as f64 * self.config.shrink_factor) as usize,
            _ => {
                let moving_ave = metrics.speed_moving();
                let perf = moving_ave.trend(self.config.speed_increase_threshold);
                match perf {
                    Trend::Improving => (current_size_bytes as f64 * self.config.growth_factor) as usize,
                    Trend::Degrading => (current_size_bytes as f64 * self.config.shrink_factor) as usize,
                    Trend::Stable => current_size_bytes,
                    Trend::Unknown => current_size_bytes,
                }
            }
        };
        let clamped = newsize.clamp(self.config.min_chunk_size, self.config.max_chunk_size);
        self.metrics_history.push(clamped as f64);
        clamped
    }

    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool {
        metrics.should_adapt(self.config.interval(metrics))
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
    pub interval: usize, // Check every N chunks
}

impl AdaptationConfig {
    pub fn interval(&self, metrics: &ChunkMetrics) -> usize {
        // Adapt more frequently early on, less frequently later
        if metrics.chunks_seen < 50 {
            5 // Adapt quickly at start
        } else if metrics.chunks_seen < 200 {
            10 // Medium frequency
        } else {
            20 // Stable operation
        }
    }
}

impl Default for AdaptationConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 8 * 1024,        // 8K
            max_chunk_size: 5 * 1024 * 1024, // Increase to 5MB
            initial_chunk_size: INITIAL_CHUNK_SIZE,
            speed_increase_threshold: 3.0,  // Lower threshold
            _speed_decrease_threshold: 0.0, // Less frequent adaptation
            memory_threshold_mb: 200,
            growth_factor: 2.0, // More aggressive growth
            shrink_factor: 0.5, // More aggressive shrink
            interval: 20,
        }
    }
}
