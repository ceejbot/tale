//! Input processors and traits related thereto.
//! I'm just getting a little fussy and neurotic about not having a million
//! files in a flat hierarchy, which is likely my problem and not anything
//! wrong with lots of files in a src directory, you know? Anyway.

mod buffered;
mod chunked;
mod single;
mod stdin;

use std::io::{self, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
pub use buffered::*;
use bytes::BytesMut;
pub use chunked::*;
pub use single::*;
pub use stdin::*;

use crate::constants::*;
use crate::{config, process_line};

/// Entry point for handling a file. We look at config and some
/// facts about the file, then pick the best processor for the circumstances.
pub fn handle_file(fpath: &Path) -> anyhow::Result<()> {
    if !fpath.exists() {
        return Err(anyhow::anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow::anyhow!("{} is not a file!", fpath.display()));
    }

    let mut processor = create_file_processor(fpath, None)?;

    // TODO smooth this out
    if let FileProcessorType::Simple(mut simple) = processor {
        // we can stop thinking because this handles everything.
        return simple.tail();
    }

    // now do what the chunked handler used to and I hate everything.
    let offset = config::offset();
    let offset_unit = config::offset_unit();

    // Handle different offset scenarios
    match (offset.is_positive(), offset_unit) {
        // Positive line offset: skip lines from start
        (true, config::OffsetUnit::Lines) if offset > 0 => {
            processor.skip_lines(offset as u64)?;

            // Process remaining lines
            let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);
            let mut outlock = io::stdout().lock();

            processor.process_lines(|line| process_line(line, &mut buffer, &mut outlock))?;

            outlock.flush()?;
        }

        // Negative line offset: seek to position and process
        (false, config::OffsetUnit::Lines) if offset < 0 => {
            // For negative offsets, we need to use the existing seek logic
            // Fall back to the original implementation for now
            let mut old_processor = SimpleFileProcessor::new(fpath.to_path_buf());
            old_processor.tail()?;
        }

        // Zero offset or other cases: process entire file
        _ => {
            let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);
            let mut outlock = io::stdout().lock();

            processor.process_lines(|line| process_line(line, &mut buffer, &mut outlock))?;

            outlock.flush()?;
        }
    }
    Ok(())
}

/// Create the optimal file processor for the given file and operation
pub fn create_file_processor<P: AsRef<Path>>(
    path: P,
    file_size_hint: Option<u64>,
) -> Result<FileProcessorType<'static>> {
    let path = path.as_ref();

    // Get file size if not provided
    let file_size = file_size_hint.unwrap_or_else(|| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));

    let offset = config::offset();
    let offset_unit = config::offset_unit();
    let large_offset = offset.abs() > 10_000; // TODO magic number

    // Pick which processor suits the situation; TODO more magic numbers
    let use_chunked = !config::disable_chunked()
        && (config::force_chunked() ||
        (file_size > 100_000_000 && large_offset) || // 100MB+ with large offset
        file_size > 1_000_000_000); // Always chunk files >1GB

    // This is the only reader that can handle negative block and byte offsets, and
    // it already handles them reasonably (though its chunks might not be
    // optimal). This is something I need to refactor away.
    if offset < 0 || matches!(offset_unit, config::OffsetUnit::Bytes | config::OffsetUnit::Blocks) {
        let processor = SimpleFileProcessor::new(PathBuf::from(path));
        return Ok(FileProcessorType::Simple(processor));
    }

    if use_chunked {
        let reader = ChunkedFileReader::with_optimal_config(path)?;
        return Ok(FileProcessorType::Chunked(reader));
    }

    let reader = BufferedFileProcessor::new(path)?;
    Ok(FileProcessorType::Buffered(reader))
}

/// Trait for abstracting different file reading strategies
pub trait FileProcessor {
    /// Process the entire file, calling the provided closure for each line
    fn process_lines<F>(&mut self, line_processor: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<()>;

    /// Skip a specified number of lines from the current position
    fn skip_lines(&mut self, count: u64) -> Result<()>;

    /// Get the file size in bytes
    fn file_size(&self) -> u64;

    /// Seek to a specific position in the file
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;

    /// Get current position in the file
    fn position(&self) -> u64;
}

/// Chonked vs buffered vs can-go-backwards variants.
pub enum FileProcessorType<'a> {
    Buffered(BufferedFileProcessor),
    Chunked(ChunkedFileReader),
    Simple(SimpleFileProcessor<'a>),
}

impl<'a> FileProcessor for FileProcessorType<'a> {
    fn process_lines<F>(&mut self, line_processor: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<()>,
    {
        match self {
            FileProcessorType::Buffered(processor) => processor.process_lines(line_processor),
            FileProcessorType::Chunked(processor) => processor.process_lines(line_processor),
            FileProcessorType::Simple(processor) => processor.process_lines(line_processor),
        }
    }

    fn skip_lines(&mut self, count: u64) -> Result<()> {
        match self {
            FileProcessorType::Buffered(processor) => processor.skip_lines(count),
            FileProcessorType::Chunked(processor) => processor.skip_lines(count),
            FileProcessorType::Simple(processor) => processor.skip_lines(count),
        }
    }

    fn file_size(&self) -> u64 {
        match self {
            FileProcessorType::Buffered(processor) => processor.file_size(),
            FileProcessorType::Chunked(processor) => processor.file_size(),
            FileProcessorType::Simple(processor) => processor.file_size(),
        }
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        match self {
            FileProcessorType::Buffered(processor) => processor.seek(pos),
            FileProcessorType::Chunked(processor) => processor.seek(pos),
            FileProcessorType::Simple(processor) => processor.seek(pos),
        }
    }

    fn position(&self) -> u64 {
        match self {
            FileProcessorType::Buffered(processor) => processor.position(),
            FileProcessorType::Chunked(processor) => processor.position(),
            FileProcessorType::Simple(processor) => processor.position(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use crate::config::ConfigOpts;

    use super::*;

    fn create_test_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes()).expect("Failed to write test data");
        file.flush().expect("Failed to flush test file");
        file
    }

    #[test]
    fn chonk_size_optimizer() {
        // Small files should use small chunks
        assert_eq!(optimal_chunk_size(500_000, None), 8_192);

        // Medium files should use medium chunks
        assert_eq!(optimal_chunk_size(50_000_000, None), 32_768);

        // Large files should use large chunks
        assert_eq!(optimal_chunk_size(500_000_000, None), 262_144);

        // Memory constraint should be respected
        assert_eq!(optimal_chunk_size(500_000_000, Some(100_000)), 10_000);
    }

    #[test]
    fn can_create_chonker() {
        let data = b"line1\nline2\nline3\n".to_vec();
        let chunk = FileChunk::new(data.clone(), 0, data.len() as u64);

        assert_eq!(chunk.size(), data.len());
        assert!(!chunk.is_empty());
        assert!(chunk.starts_at_line_boundary);
        assert!(chunk.ends_at_line_boundary);
    }

    #[test]
    fn test_chunked_file_reader_small_file() -> Result<()> {
        let test_data = "line1\nline2\nline3\n";
        let temp_file = create_test_file(test_data);

        let config = ChunkConfig {
            chunk_size: 8, // Small chunks to test boundary handling
            overlap_size: 2,
            low_memory_mode: true,
        };

        let mut reader = ChunkedFileReader::new(temp_file.path(), config)?;

        assert_eq!(reader.file_size(), test_data.len() as u64);
        assert_eq!(reader.position(), 0);
        assert!(!reader.is_at_end());

        // Read chunks and verify content
        let mut all_content = String::new();
        while let Some(chunk) = reader.read_chunk()? {
            let chunk_str = std::str::from_utf8(&chunk.data).unwrap();
            all_content.push_str(chunk_str);
        }

        assert_eq!(all_content, test_data);
        assert!(reader.is_at_end());

        Ok(())
    }

    #[test]
    fn line_boundary_handling() {
        let data = b"line1\nline2\npartial".to_vec();
        let data_len = data.len();
        let mut chunk = FileChunk::new(data, 0, data_len as u64);

        assert!(!chunk.ends_at_line_boundary);

        let remainder = chunk.split_at_last_line();
        assert!(remainder.is_some());
        assert_eq!(remainder.unwrap(), b"partial");
        assert_eq!(chunk.data, b"line1\nline2\n");
        assert!(chunk.ends_at_line_boundary);
    }

    #[test]
    fn chunk_iterator_works() {
        let data = b"line1\nline2\nline3".to_vec();
        let data_len = data.len();
        let chunk = FileChunk::new(data, 0, data_len as u64);

        let lines: Vec<&str> = chunk.lines().collect();
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn buffer_thing_works() -> Result<()> {
        let test_data = "line1\nline2\nline3\n";
        let temp_file = create_test_file(test_data);

        let mut processor = BufferedFileProcessor::new(temp_file.path())?;

        assert_eq!(processor.file_size(), test_data.len() as u64);
        assert_eq!(processor.position(), 0);

        let mut lines = Vec::new();
        processor.process_lines(|line| {
            lines.push(line.to_string());
            Ok(())
        })?;

        assert_eq!(lines, vec!["line1", "line2", "line3"]);
        Ok(())
    }

    #[test]
    fn abstract_processor_impl_factory_noun() -> Result<()> {
        let cfg = ConfigOpts::default();
        config::set(cfg).expect("the test should be able to set config");

        let test_data = "line1\nline2\nline3\n";
        let temp_file = create_test_file(test_data);

        // Small file should use buffered processor
        let mut processor = create_file_processor(temp_file.path(), None)?;
        assert_eq!(processor.file_size(), test_data.len() as u64);

        let mut lines = Vec::new();
        processor.process_lines(|line| {
            lines.push(line.to_string());
            Ok(())
        })?;

        assert_eq!(lines, vec!["line1", "line2", "line3"]);
        Ok(())
    }
}
