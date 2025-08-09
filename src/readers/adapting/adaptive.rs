use crate::constants::INITIAL_CHUNK_SIZE;
/// An adaptive chunked reader, which uses its current strategy
/// to decide how to read incoming data. Look, don't complain that I'm
/// overengineering a hobby project. The whole point of a hobby project
/// is learning through overengineering. And why are you reading this
/// comment, anyway?
use crate::errors::TaleError;
use crate::readers::{ChunkedFileReader, FileProcessor};

use super::ChunkStrategy;
use super::MemoryPressure;
use super::metrics::{ChunkMetrics, MovingAverage};

/*
The adaptive algo works like this:

Every N chunks:
1. Collect metrics (speed, memory, I/O wait)
2. Calculate performance trend (moving average)
3. Check memory pressure
4. Decision tree:
   - If high memory pressure → shrink by shrink_factor
   - Else if speed improving & memory ok → grow by growth_factor
   - Else if speed degrading → shrink to find sweet spot
   - Else → maintain current size
5. Apply bounds (min_chunk_size, max_chunk_size)
6. Update metrics history

What is N?
*/

#[derive(Debug, Clone)]
pub struct AdaptiveStrategy {
    config: AdaptationConfig,
    metrics_history: MovingAverage<10>,
    chunks_since_adaptation: usize,
}

impl Default for AdaptiveStrategy {
    fn default() -> Self {
        let config = AdaptationConfig::default();
        Self {
            config,
            metrics_history: MovingAverage::new(),
            chunks_since_adaptation: 0,
        }
    }
}

// Implements the decision tree you outlined
impl ChunkStrategy for AdaptiveStrategy {
    fn initial_chunk_size(&self) -> usize {
        crate::constants::INITIAL_CHUNK_SIZE
    }

    fn adapt_size(&mut self, metrics: &ChunkMetrics, current_size: usize) -> usize {
        let pressure = super::memory::detect_memory_pressure();
        match pressure {
            MemoryPressure::Critical => {
                // TODO shrink to minimum
                // return
            }
            MemoryPressure::High => {
                // shrink by shrink factor
                // return
            }
            _ => {
                // look at perf trend
            }
        }
        /*
        Decision Tree Logic:
        1. Get memory pressure
        2. If Critical → shrink to minimum
        3. If High → shrink by shrink_factor
        4. Else, check performance trend:
           - If improving → grow by growth_factor
           - If degrading → shrink by shrink_factor
           - If stable → no change
        5. Apply bounds (min/max)
        */
        todo!()
    }

    fn should_adapt(&self, metrics: &ChunkMetrics) -> bool {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct AdaptationConfig {
    // Size boundaries (in bytes)
    pub min_chunk_size: usize,     // Don't go below this (e.g., 4KB)
    pub max_chunk_size: usize,     // Don't exceed this (e.g., 1MB)
    pub initial_chunk_size: usize, // Starting point (e.g., 32KB)

    // When to grow/shrink
    pub speed_increase_threshold: f64, // If speed improves by X%, grow chunks
    pub speed_decrease_threshold: f64, // If speed drops by X%, shrink chunks
    pub memory_threshold_mb: usize,    // Shrink if we use more than this

    // How much to grow/shrink
    pub growth_factor: f64, // Multiply size by this (e.g., 1.5)
    pub shrink_factor: f64, // Multiply size by this (e.g., 0.67)

    // How often to check
    pub adaptation_interval: usize, // Check every N chunks (e.g., 5)
}

impl Default for AdaptationConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 4 * 1024,    // 4K bytes
            max_chunk_size: 1024 * 1024, // 1MB
            initial_chunk_size: INITIAL_CHUNK_SIZE,
            speed_increase_threshold: Default::default(),
            speed_decrease_threshold: Default::default(),
            memory_threshold_mb: Default::default(),
            growth_factor: Default::default(),
            shrink_factor: Default::default(),
            adaptation_interval: Default::default(),
        }
    }
}

/// An adaptive input processor.
#[derive(Debug)]
pub struct AdaptiveProcessor {
    // TODO
    /// We wrap a chunked file reader.
    chonker: ChunkedFileReader,
}

/// We must implement this.
impl FileProcessor for AdaptiveProcessor {
    fn process_lines<F>(&mut self, _line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        todo!()
    }

    fn skip_lines(&mut self, _count: u64) -> Result<(), TaleError> {
        todo!()
    }

    fn file_size(&self) -> u64 {
        todo!()
    }

    fn seek(&mut self, _pos: std::io::SeekFrom) -> Result<u64, miette::Error> {
        todo!()
    }

    fn position(&self) -> u64 {
        todo!()
    }
}
