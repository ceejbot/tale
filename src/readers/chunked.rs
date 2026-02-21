//! Memory-efficient chunked file processing.
//!
//! `ChunkedFileReader` processes large files in manageable chunks while
//! maintaining:
//! - **Bounded memory usage**: Memory footprint independent of file size
//! - **Line boundary handling**: Proper JSON line parsing across chunks
//!
//! ## Architecture
//! - StaticStrategy determines chunk size based on file size
//! - ChunkConfig holds immutable settings (overlap_size, low_memory_mode)
//! - FileChunk manages data boundaries and line parsing

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::FileProcessor;
use crate::errors::TaleError;
use crate::readers::strategies::{ChunkConfig, StaticStrategy};

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
    /// Chunk sizing strategy
    strategy: StaticStrategy,
}

impl ChunkedFileReader {
    /// Create a new ChunkedFileReader
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let file_size = std::fs::metadata(&path)?.len();

        // Get strategy from global config
        #[cfg(not(test))]
        let strategy = StaticStrategy::from_config(crate::config::config(), Some(file_size));
        #[cfg(test)]
        let strategy = StaticStrategy::from_config(&crate::config::config(), Some(file_size));

        let path = path.as_ref().to_path_buf();

        let mut file = File::open(&path)?;
        file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;

        Ok(Self {
            file,
            file_size,
            current_position: 0,
            _path: path,
            pending_data: Vec::new(),
            strategy,
        })
    }

    /// Create with explicit strategy (for testing)
    pub fn with_strategy<P: AsRef<Path>>(path: P, strategy: StaticStrategy) -> Result<Self, TaleError> {
        let mut reader = Self::new(path)?;
        reader.strategy = strategy;
        Ok(reader)
    }

    /// Pick the optimal chunk size and stick with it
    pub fn static_optimal<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let mut reader = Self::new(&path)?;
        let file_size = reader.file_size;
        let strategy = StaticStrategy::optimal_for_file(file_size);
        reader.strategy = strategy;
        Ok(reader)
    }

    pub fn new_with_config<P: AsRef<Path>>(path: P, config: ChunkConfig) -> Result<Self, TaleError> {
        let mut reader = Self::new(&path)?;
        let strategy = StaticStrategy::with_config(config);
        reader.strategy = strategy;
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

        let chunk_size = self.strategy.initial_chunk_size();

        // Track how much pending data we have at the start
        let pending_len = self.pending_data.len();

        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = if self.is_at_end() {
            // At EOF, just process any pending data
            0
        } else {
            // Read new data from file
            self.file.read(&mut buffer).map_err(TaleError::from)?
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
                        }
                    } else {
                        // No newlines in this chunk, keep the entire chunk as pending
                        self.pending_data = chunk.data;
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
