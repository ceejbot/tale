//! Chunked file processing for memory-efficient handling of large files.
//!
//! This exists so we can:
//! - cope with large files with constant memory usage
//! - handle large offsets better than before
//! - do parallel processing in the future
//! - operate in memory-constrained situations

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use miette::{ErrReport, Result};

use super::FileProcessor;
use super::adapting::*;
use crate::constants::READ_BUFFER_SIZE;
use crate::errors::TaleError;
use crate::readers::Strategy;

/// Configuration for FileChunk processing
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Size of each chunk in bytes
    pub chunk_size: usize,
    /// Maximum overlap between chunks to handle line boundaries
    pub overlap_size: usize,
    /// Whether to use memory-constrained processing
    pub low_memory_mode: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: READ_BUFFER_SIZE,
            overlap_size: 1024, // 1KB overlap for line boundaries
            low_memory_mode: false,
        }
    }
}

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

/// Determine optimal chunk size based on file characteristics
pub fn optimal_chunk_size(file_size: u64, available_memory: Option<usize>) -> usize {
    let default_memory = 10 * 1024 * 1024; // 10MB default
    let memory_limit = available_memory.unwrap_or(default_memory);

    match file_size {
        // Small files: use small chunks to minimize overhead
        0..=1_000_000 => 8_192, // 8KB

        // Medium files: balance memory and I/O efficiency
        1_000_001..=100_000_000 => 32_768, // 32KB

        // Large files: use large chunks but respect memory limits
        _ => std::cmp::min(262_144, memory_limit / 10), // 256KB max, or 10% of available memory
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
    /// Current chunk configuration
    config: ChunkConfig,
    /// The path of the file we're reading from
    path: PathBuf,
    /// Data from previous chunk that didn't end at line boundary
    pending_data: Vec<u8>,
    /// What's the strategy, Kenneth?
    strategy: Strategy,
    /// Tracking how we're doing
    metrics: ChunkMetrics,
}

impl ChunkedFileReader {
    /// Create a new ChunkedFileReader
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let file_size = std::fs::metadata(&path)?.len();
        let initial_size = optimal_chunk_size(file_size, None);

        let config = ChunkConfig {
            chunk_size: initial_size,
            overlap_size: 1024,
            low_memory_mode: false,
        };

        let strategy = Strategy::pick_strategy();
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(TaleError::from)?;
        let file_size = file.seek(SeekFrom::End(0)).map_err(TaleError::from)?;

        file.seek(SeekFrom::Start(0)).map_err(TaleError::from)?;

        Ok(Self {
            file,
            file_size,
            current_position: 0,
            config,
            path,
            pending_data: Vec::new(),
            strategy,
            metrics: ChunkMetrics::new(),
        })
    }

    pub fn new_with_config<P: AsRef<Path>>(path: P, config: ChunkConfig) -> Result<Self, TaleError> {
        // Always adaptive unless in constrained environment
        let strategy = Strategy::pick_strategy();

        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(TaleError::from)?;
        let file_size = file.seek(SeekFrom::End(0)).map_err(TaleError::from)?;

        file.seek(SeekFrom::Start(0)).map_err(TaleError::from)?;

        Ok(Self {
            file,
            file_size,
            current_position: 0,
            config,
            path,
            pending_data: Vec::new(),
            strategy,
            metrics: ChunkMetrics::new(),
        })
    }

    /// Create a ChunkedFileReader with optimal configuration for the file
    pub fn with_optimal_config<P: AsRef<Path>>(path: P) -> Result<Self, ErrReport> {
        let path = path.as_ref().to_path_buf();
        let file_size = std::fs::metadata(&path).map_err(TaleError::from)?.len();

        let chunk_size = optimal_chunk_size(file_size, None);
        let config = ChunkConfig {
            chunk_size,
            overlap_size: std::cmp::min(1024, chunk_size / 8), // Adaptive overlap
            low_memory_mode: chunk_size <= 8192,
        };

        let mut reader = Self::new(path)?;
        reader.config = config;
        Ok(reader)
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
    pub fn read_chunk(&mut self) -> Result<Option<FileChunk>> {
        if self.is_at_end() {
            return Ok(None);
        }

        // Adapt chunk size if needed
        match &mut self.strategy {
            Strategy::Adaptive(adaptive) => {
                if adaptive.should_adapt(&self.metrics) {
                    let new_size = adaptive.adapt_size(&self.metrics, self.config.chunk_size);
                    self.config.chunk_size = new_size;
                }
            }
            Strategy::Static(_) => {
                // No adaptation
            }
            Strategy::Conservative(_strategy) => {
                // Conservative adaptation - only shrink, never grow
                if self.metrics.chunks_seen % 10 == 0 {
                    // Less frequent
                    let pressure = detect_memory_pressure(None);
                    if matches!(pressure, MemoryPressure::High | MemoryPressure::Critical) {
                        self.config.chunk_size = (self.config.chunk_size / 2).max(4096);
                    }
                }
            }
        }

        let mut buffer = vec![0u8; self.config.chunk_size];
        let start = std::time::Instant::now();
        let bytes_read = self.file.read(&mut buffer).map_err(TaleError::from)?;
        let read_duration = start.elapsed();

        if bytes_read == 0 {
            return Ok(None);
        }

        buffer.truncate(bytes_read);

        // Combine with any pending data from previous chunk
        if !self.pending_data.is_empty() {
            let mut combined = std::mem::take(&mut self.pending_data);
            combined.extend_from_slice(&buffer);
            buffer = combined;
        }

        let start_offset = self.current_position - self.pending_data.len() as u64;
        self.current_position += bytes_read as u64;

        let mut chunk = FileChunk::new(buffer, start_offset, self.current_position);

        // Handle line boundaries: if chunk doesn't end at a line boundary,
        // save the partial line for the next chunk
        if !chunk.ends_at_line_boundary && !self.is_at_end() {
            if let Some(remainder) = chunk.split_at_last_line() {
                self.pending_data = remainder;
            }
        }

        // Count lines in the chunk for metrics
        let line_count = chunk.data.iter().filter(|&&b| b == b'\n').count() as u64;

        // Record metrics
        self.metrics
            .record_chunk_processing(chunk.size(), read_duration, line_count as usize);
        Ok(Some(chunk))
    }

    /// Seek to a specific position in the file
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64, ErrReport> {
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

                for (i, &byte) in chunk.data.iter().enumerate() {
                    if byte == b'\n' {
                        lines_in_chunk += 1;

                        // Check if we've skipped enough lines
                        if lines_skipped + lines_in_chunk == count {
                            // We need to keep the rest of this chunk for processing
                            // Save the unprocessed portion as pending data
                            let position_after_newline = i + 1;
                            if position_after_newline < chunk.data.len() {
                                self.pending_data = chunk.data[position_after_newline..].to_vec();
                            }
                            return Ok(());
                        }
                    }
                }

                // Entire chunk was consumed
                lines_skipped += lines_in_chunk;
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

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, ErrReport> {
        self.seek(pos)
    }

    fn position(&self) -> u64 {
        self.current_position
    }
}
