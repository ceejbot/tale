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

use anyhow::{Context, Result};

use super::FileProcessor;
use crate::constants::READ_BUFFER_SIZE;

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
pub struct ChunkedFileReader {
    file: File,
    file_size: u64,
    current_position: u64,
    config: ChunkConfig,
    path: PathBuf,
    pending_data: Vec<u8>, // Data from previous chunk that didn't end at line boundary
}

impl ChunkedFileReader {
    /// Create a new ChunkedFileReader
    pub fn new<P: AsRef<Path>>(path: P, config: ChunkConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).with_context(|| format!("Failed to open file: {}", path.display()))?;

        let file_size = file
            .seek(SeekFrom::End(0))
            .with_context(|| "Failed to determine file size")?;

        file.seek(SeekFrom::Start(0))
            .with_context(|| "Failed to seek to start of file")?;

        Ok(Self {
            file,
            file_size,
            current_position: 0,
            config,
            path,
            pending_data: Vec::new(),
        })
    }

    /// Create a ChunkedFileReader with optimal configuration for the file
    pub fn with_optimal_config<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file_size = std::fs::metadata(&path)
            .with_context(|| format!("Failed to get file metadata: {}", path.display()))?
            .len();

        let chunk_size = optimal_chunk_size(file_size, None);
        let config = ChunkConfig {
            chunk_size,
            overlap_size: std::cmp::min(1024, chunk_size / 8), // Adaptive overlap
            low_memory_mode: chunk_size <= 8192,
        };

        Self::new(path, config)
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

        let chunk_size = std::cmp::min(
            self.config.chunk_size,
            (self.file_size - self.current_position) as usize,
        );

        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = self
            .file
            .read(&mut buffer)
            .with_context(|| "Failed to read chunk from file")?;

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

        Ok(Some(chunk))
    }

    /// Seek to a specific position in the file
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let new_pos = self.file.seek(pos).with_context(|| "Failed to seek in file")?;

        self.current_position = new_pos;
        self.pending_data.clear(); // Clear pending data after seek

        Ok(new_pos)
    }

    /// Reset to the beginning of the file
    pub fn reset(&mut self) -> Result<()> {
        self.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

impl FileProcessor for ChunkedFileReader {
    fn process_lines<F>(&mut self, mut line_processor: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<()>,
    {
        while let Some(chunk) = self.read_chunk()? {
            for line in chunk.lines() {
                line_processor(line)?;
            }
        }
        Ok(())
    }

    fn skip_lines(&mut self, count: u64) -> Result<()> {
        let mut lines_skipped = 0u64;
        while lines_skipped < count {
            if let Some(chunk) = self.read_chunk()? {
                let chunk_lines: Vec<&str> = chunk.lines().collect();
                let lines_in_chunk = chunk_lines.len() as u64;

                if lines_skipped + lines_in_chunk <= count {
                    // Skip entire chunk
                    lines_skipped += lines_in_chunk;
                } else {
                    // Skip partial chunk - we need to be more careful here
                    // For now, we'll process the remaining lines individually
                    let lines_to_skip_in_chunk = count - lines_skipped;

                    // Process remaining lines in this chunk
                    for _line in chunk_lines.iter().skip(lines_to_skip_in_chunk as usize) {
                        // TODO this implementation does not consume the lines

                        // This approach isn't optimal - in a real implementation,
                        // we'd want to adjust the file position more precisely
                        break;
                    }
                    break;
                }
            } else {
                break; // EOF
            }
        }
        Ok(())
    }

    fn file_size(&self) -> u64 {
        self.file_size
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.seek(pos)
    }

    fn position(&self) -> u64 {
        self.current_position
    }
}
