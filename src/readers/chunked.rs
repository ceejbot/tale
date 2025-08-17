//! Memory-efficient chunked file processing with strategy-based adaptation.
//!
//! `ChunkedFileReader` processes large files in manageable chunks while
//! maintaining:
//! - **Bounded memory usage**: Memory footprint independent of file size
//! - **Line boundary handling**: Proper JSON line parsing across chunks
//! - **Adaptive performance**: Strategy pattern for chunk size optimization
//! - **Metrics collection**: Performance tracking for adaptation decisions
//!
//! ## Architecture
//! - Strategy owns chunk_size (StaticStrategy, AdaptiveStrategy,
//!   ConservativeStrategy)
//! - ChunkConfig holds immutable settings (overlap_size, low_memory_mode)
//! - FileChunk manages data boundaries and line parsing
//! - ChunkMetrics tracks performance for adaptive strategies
//!
//! ## Usage
//! ```
//! let mut reader = ChunkedFileReader::new(&path)?;
//! reader.process_lines(|line| { /* process line */ Ok(()) })?;
//! ```

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::FileProcessor;
use super::strategies::Strategy;
use crate::errors::TaleError;
use crate::memory_budget::{MemoryAllocation, MemoryBudget, MemoryPressure};
use crate::metrics::*;
use crate::readers::strategies::ChunkConfig;
use crate::readers::{IsStrategy, StaticStrategy};

/// A chunk of file data with metadata about its position
#[derive(Debug)]
pub struct FileChunk {
    /// The chunk data
    pub data: Vec<u8>,
    /// Starting position in the file
    pub start_offset: u64,
    /// End position in the file (exclusive)
    pub end_offset: u64,
    /// Whether this chunk starts at a line boundary
    pub starts_at_line_boundary: bool,
    /// Whether this chunk ends at a line boundary
    pub ends_at_line_boundary: bool,
}

impl FileChunk {
    /// Create a new FileChunk
    pub fn new(data: Vec<u8>, start_offset: u64, end_offset: u64) -> Self {
        let starts_at_line_boundary = start_offset == 0 || data.first() != Some(&b'\n');
        let ends_at_line_boundary = data.last() == Some(&b'\n');

        Self {
            data,
            start_offset,
            end_offset,
            starts_at_line_boundary,
            ends_at_line_boundary,
        }
    }

    /// Get the size of this chunk
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Check if this chunk is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get lines from this chunk, handling partial lines
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        let data_str = std::str::from_utf8(&self.data).unwrap_or("");
        data_str.lines()
    }

    /// Find the last complete line boundary in this chunk
    pub fn find_last_line_boundary(&self) -> Option<usize> {
        self.data.iter().rposition(|&b| b == b'\n')
    }

    /// Split this chunk at the last complete line boundary
    pub fn split_at_last_line(&mut self) -> Option<Vec<u8>> {
        if let Some(boundary) = self.find_last_line_boundary() {
            let remainder = self.data.split_off(boundary + 1);
            self.end_offset = self.start_offset + self.data.len() as u64;
            self.ends_at_line_boundary = true;
            Some(remainder)
        } else {
            None
        }
    }
}

/// Reader that processes files in chunks with line boundary handling
#[derive(Debug)]
pub struct ChunkedFileReader {
    /// An open file pointer we're reading from.
    file: File,
    /// The size of the file we're reading.
    file_size: u64,
    /// Our current position in the file we're reading.
    current_position: u64,
    /// The path of the file we're reading from
    _path: PathBuf,
    /// Data from previous chunk that didn't end at line boundary
    pending_data: Vec<u8>,
    /// What's the strategy, Kenneth?
    strategy: Strategy,
    /// Tracking how we're doing
    metrics: ChunkMetrics,
    /// Memory budget management
    memory_budget: Option<MemoryBudget>,
    /// Current chunk allocation
    current_allocation: Option<MemoryAllocation>,
    /// Reader ID for memory tracking
    reader_id: String,
}

impl ChunkedFileReader {
    /// Create a new ChunkedFileReader
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let file_size = std::fs::metadata(&path)?.len();

        // Get strategy from global config
        #[cfg(not(test))]
        let strategy = Strategy::from_config(crate::config::config(), Some(file_size));
        #[cfg(test)]
        let strategy = Strategy::from_config(&crate::config::config(), Some(file_size));

        let path = path.as_ref().to_path_buf();
        let reader_id = format!(
            "chunked_reader_{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown")
        );

        let mut file = File::open(&path)?;
        file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;

        // Try to create memory budget from config max_memory
        let memory_budget = if let Some(max_memory) = crate::config::config().max_memory {
            Some(MemoryBudget::new(max_memory)?)
        } else {
            // Default: use 10% of system memory
            MemoryBudget::from_system_memory(10.0).ok()
        };

        Ok(Self {
            file,
            file_size,
            current_position: 0,
            _path: path,
            pending_data: Vec::new(),
            strategy,
            metrics: ChunkMetrics::new(),
            memory_budget,
            current_allocation: None,
            reader_id,
        })
    }

    /// Create with explicit strategy (for testing)
    pub fn with_strategy<P: AsRef<Path>>(path: P, strategy: Strategy) -> Result<Self, TaleError> {
        let mut reader = Self::new(path)?;
        reader.strategy = strategy;
        Ok(reader)
    }

    /// Create with explicit memory budget
    pub fn with_memory_budget<P: AsRef<Path>>(path: P, memory_budget: MemoryBudget) -> Result<Self, TaleError> {
        let mut reader = Self::new(path)?;
        reader.memory_budget = Some(memory_budget);
        Ok(reader)
    }

    /// Pick the optimal chunk size and stick with it
    pub fn static_optimal<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let mut reader = Self::new(&path)?;
        let file_size = reader.file_size;
        let strategy = StaticStrategy::optimal_for_file(file_size);
        reader.strategy = Strategy::Static(strategy);
        Ok(reader)
    }

    pub fn new_with_config<P: AsRef<Path>>(path: P, config: ChunkConfig) -> Result<Self, TaleError> {
        let mut reader = Self::new(&path)?;
        let strategy = StaticStrategy::with_config(config);
        reader.strategy = Strategy::Static(strategy);
        Ok(reader)
    }

    /// Create a ChunkedFileReader with optimal configuration for the file
    pub fn with_optimal_config<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        Self::static_optimal(path)
    }

    /// Get the file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get current position in file
    pub fn position(&self) -> u64 {
        self.current_position
    }

    /// Check if we've reached the end of file
    pub fn is_at_end(&self) -> bool {
        self.current_position >= self.file_size
    }

    /// Read the next chunk from the file
    pub fn read_chunk(&mut self) -> Result<Option<FileChunk>, TaleError> {
        // If we have pending data, we need to process it even if we're at EOF
        if self.is_at_end() && self.pending_data.is_empty() {
            return Ok(None);
        }

        // Let strategy adapt if needed
        if self.metrics.chunks_seen % crate::defaults::processing::ADAPTATION_INTERVAL == 0
            && self.strategy.should_adapt(&self.metrics)
        {
            let current_size = self.strategy.initial_chunk_size();
            self.strategy.adapt_size(&self.metrics, current_size);
        }

        // Get base chunk size from strategy
        let mut chunk_size = self.strategy.initial_chunk_size();

        // Apply memory budget adjustments if available
        if let Some(ref budget) = self.memory_budget {
            // Check memory pressure and adjust chunk size
            if let Ok(pressure) = budget.current_pressure() {
                let factor = pressure.chunk_size_factor();
                chunk_size = (chunk_size as f64 * factor) as usize;

                // Don't let it get too small
                chunk_size = chunk_size.max(4096); // Minimum 4KB

                // Log critical memory pressure
                if matches!(pressure, MemoryPressure::Critical) {
                    eprintln!(
                        "⚠️  Critical memory pressure - reducing chunk size to {} bytes",
                        chunk_size
                    );
                }
            }

            // Try to allocate memory for this chunk
            let total_allocation_needed = chunk_size + self.pending_data.len();

            // Release previous allocation first
            self.current_allocation = None;

            // Try to allocate new chunk
            match budget.try_allocate(total_allocation_needed, &self.reader_id) {
                Ok(Some(allocation)) => {
                    self.current_allocation = Some(allocation);
                }
                Ok(None) => {
                    // Allocation failed - try with smaller chunk size
                    let emergency_size = chunk_size / 4; // Emergency 25% size
                    if emergency_size >= 1024 {
                        // Don't go below 1KB
                        chunk_size = emergency_size;
                        let emergency_allocation =
                            budget.try_allocate(emergency_size + self.pending_data.len(), &self.reader_id)?;
                        if let Some(allocation) = emergency_allocation {
                            self.current_allocation = Some(allocation);
                            eprintln!("🆘 Emergency memory allocation - using {} byte chunks", chunk_size);
                        } else {
                            return Err(TaleError::MemoryError(
                                "Cannot allocate memory even for emergency chunk size".to_string(),
                            ));
                        }
                    } else {
                        return Err(TaleError::MemoryError(
                            "Out of memory - chunk size would be too small".to_string(),
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
        }

        // Track how much pending data we have at the start
        let pending_len = self.pending_data.len();

        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = if self.is_at_end() {
            // At EOF, just process any pending data
            0
        } else {
            // Read new data from file
            let start = std::time::Instant::now();
            let read = self.file.read(&mut buffer).map_err(TaleError::from)?;
            let read_duration = start.elapsed();

            // Record read metrics
            if read > 0 {
                let line_count = buffer[..read].iter().filter(|&&b| b == b'\n').count();
                self.metrics.record_chunk_processing(read, read_duration, line_count);
            }

            read
        };

        if bytes_read == 0 && self.pending_data.is_empty() {
            return Ok(None);
        }

        buffer.truncate(bytes_read);

        // Combine with any pending data from previous chunk
        if !self.pending_data.is_empty() {
            let mut combined = std::mem::take(&mut self.pending_data);
            combined.extend_from_slice(&buffer);
            buffer = combined;
        }

        let start_offset = self.current_position - pending_len as u64;
        self.current_position += bytes_read as u64;

        let mut chunk = FileChunk::new(buffer, start_offset, self.current_position);

        // Handle line boundaries: if chunk doesn't end at a line boundary,
        // save the partial line for the next chunk
        if !chunk.ends_at_line_boundary
            && !self.is_at_end()
            && let Some(remainder) = chunk.split_at_last_line()
        {
            self.pending_data = remainder;
        }

        // Metrics are now recorded earlier when we actually read from file

        Ok(Some(chunk))
    }

    /// Seek to a specific position in the file
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64, TaleError> {
        let new_pos = self.file.seek(pos).map_err(TaleError::from)?;

        self.current_position = new_pos;
        // we've moved and no longer care what we read earlier
        self.pending_data.clear();

        Ok(new_pos)
    }

    /// Reset to the beginning of the file
    pub fn reset(&mut self) -> Result<(), TaleError> {
        self.seek(SeekFrom::Start(0))?;
        Ok(())
    }

    /// Get current memory pressure level
    pub fn memory_pressure(&self) -> Option<Result<MemoryPressure, TaleError>> {
        self.memory_budget.as_ref().map(|budget| budget.current_pressure())
    }

    /// Get memory budget statistics
    pub fn memory_stats(&self) -> Option<Result<crate::memory_budget::MemoryBudgetStats, TaleError>> {
        self.memory_budget.as_ref().map(|budget| budget.usage_stats())
    }

    /// Check if memory budget is active
    pub fn has_memory_budget(&self) -> bool {
        self.memory_budget.is_some()
    }
}

impl FileProcessor for ChunkedFileReader {
    fn process_lines<F>(&mut self, mut line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        while let Some(chunk) = self.read_chunk()? {
            for line in chunk.lines() {
                line_processor(line)?;
            }
        }
        Ok(())
    }

    fn skip_lines(&mut self, count: u64) -> Result<(), TaleError> {
        let mut lines_skipped = 0u64;

        while lines_skipped < count {
            if let Some(chunk) = self.read_chunk()? {
                // Count lines in this chunk and track position
                let mut lines_in_chunk = 0u64;
                let mut last_newline_pos = None;

                for (i, &byte) in chunk.data.iter().enumerate() {
                    if byte == b'\n' {
                        lines_in_chunk += 1;
                        last_newline_pos = Some(i);

                        // Check if we've skipped enough lines
                        if lines_skipped + lines_in_chunk == count {
                            // We need to keep the rest of this chunk for processing
                            // Save the unprocessed portion as pending data
                            let position_after_newline = i + 1;
                            if position_after_newline < chunk.data.len() {
                                self.pending_data = chunk.data[position_after_newline..].to_vec();
                                // Note: We don't adjust current_position here
                                // because read_chunk
                                // already handles the position tracking
                                // correctly with pending_data
                            }
                            return Ok(());
                        }
                    }
                }

                // Entire chunk was consumed
                lines_skipped += lines_in_chunk;

                // If this chunk didn't end with a newline and we haven't skipped enough lines
                // yet, we need to keep any partial line for the next iteration
                if !chunk.ends_at_line_boundary && lines_skipped < count {
                    if let Some(last_nl) = last_newline_pos {
                        // Keep everything after the last newline as pending data
                        let after_last_newline = last_nl + 1;
                        if after_last_newline < chunk.data.len() {
                            self.pending_data = chunk.data[after_last_newline..].to_vec();
                            // Note: We don't adjust current_position here
                        }
                    } else {
                        // No newlines in this chunk, keep the entire chunk as pending
                        self.pending_data = chunk.data;
                        // Note: We don't adjust current_position here
                    }
                }
            } else {
                // EOF reached before skipping all requested lines
                break;
            }
        }

        Ok(())
    }

    fn file_size(&self) -> u64 {
        self.file_size
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, TaleError> {
        self.seek(pos)
    }

    fn position(&self) -> u64 {
        self.current_position
    }
}
