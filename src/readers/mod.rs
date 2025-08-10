//! File processing architecture with different strategies for various use cases.
//!
//! This module provides a clean hierarchy of file processors:
//!
//! ## Processors
//! - **BufferedFileProcessor**: Simple forward-only reading for small files
//! - **ChunkedFileReader**: Memory-efficient processing with Strategy-based adaptation
//! - **BackSeekingProcessor**: Handles backward seeking and tail-like functionality
//!
//! ## Strategy Pattern
//! ChunkedFileReader uses the Strategy pattern for chunk size management:
//! - **StaticStrategy**: Fixed optimal chunk size (fastest, predictable memory)
//! - **AdaptiveStrategy**: Dynamic sizing based on performance metrics
//! - **ConservativeStrategy**: Memory-constrained environments
//!
//! ## Processor Selection
//! `create_file_processor()` automatically selects the best processor based on:
//! - File size and offset requirements
//! - Memory constraints (--chunked, --no-chunked flags)
//! - Operation type (negative offsets → BackSeekingProcessor)
//!
//! ## Architecture Benefits
//! - **Single source of truth**: Strategy owns chunk_size (not ChunkConfig)
//! - **Memory efficiency**: Bounded memory usage regardless of file size
//! - **Performance**: Adaptive strategies optimize for throughput vs memory
//! - **Extensibility**: Easy to add new strategies or processors

mod backseeking;
mod buffered;
mod chunked; // the file processor
mod stdin;
mod strategies; // the chunking strategies; will figure out naming later

use std::io::{self, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub use backseeking::*;
pub use buffered::*;
use bytes::BytesMut;
pub use chunked::*;
use miette::ErrReport;
pub use stdin::*;
pub use strategies::*;

use crate::constants::*;
use crate::errors::{FileError, TaleError, find_similar_files};
use crate::{config, process_line};

/// We're displaying a file. Let's chug through it.
pub fn handle_file(fpath: &Path) -> Result<(), TaleError> {
    let _sticky = config::sticky();

    // Check if file exists and provide helpful suggestions
    if !fpath.exists() {
        // TODO if we are sticky, we need to watch the file until it
        // appears or the user kills the process.
        let similar_files = find_similar_files(fpath);
        return Err(Box::new(FileError::not_found_with_suggestions(
            fpath.to_path_buf(),
            similar_files,
        ))
        .into());
    }

    // Check if it's actually a file (not a directory, etc.)
    if !fpath.is_file() {
        return Err(Box::new(FileError::not_a_file_with_type(fpath.to_path_buf())).into());
    }

    // Try to create processor and provide helpful context for common errors
    let mut processor = create_file_processor(fpath, None).map_err(|e| enhance_error_context(e, fpath))?;

    // BackSeekingProcessor handles its own special cases (negative offsets, bytes,
    // blocks)
    if let FileProcessorType::BackSeeking(mut backseeker) = processor {
        return backseeker.tail();
    }

    // For buffered and chunked processors, handle offset scenarios
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

fn enhance_error_context(error: TaleError, path: &Path) -> TaleError {
    match error {
        TaleError::Io(io_error) => {
            let crate::errors::IoError::OperationFailed { source, .. } = io_error.as_ref();
            if source.kind() == std::io::ErrorKind::PermissionDenied {
                let suggestion = if cfg!(unix) {
                    Some(format!("Try: chmod +r {}", path.display()))
                } else {
                    Some("Check file permissions in Properties".to_string())
                };
                return Box::new(FileError::permission_denied_with_suggestion(
                    path.to_path_buf(),
                    suggestion,
                ))
                .into();
            } else {
                return TaleError::Io(io_error);
            }
        }
        other => other,
    }
}

/// Create the optimal file processor for the given file and operation
pub fn create_file_processor<P: AsRef<Path>>(
    path: P,
    file_size_hint: Option<u64>,
) -> Result<FileProcessorType<'static>, TaleError> {
    let path = path.as_ref();
    let _strategy = if cfg!(debug_assertions) && config::conservative() {
        Strategy::Static(StaticStrategy::conservative())
    } else {
        // Normal smart adaptation
        Strategy::default()
    };

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
        let processor = BackSeekingProcessor::new(PathBuf::from(path));
        return Ok(FileProcessorType::BackSeeking(processor));
    }

    if use_chunked {
        let reader = ChunkedFileReader::with_optimal_config(path)?;
        return Ok(FileProcessorType::Chunked(Box::new(reader)));
    }

    // We'll get here if we have a positive by-lines offset.
    let reader = BufferedFileProcessor::new(path)?;
    Ok(FileProcessorType::Buffered(reader))
}

/// Trait for abstracting different file reading strategies
pub trait FileProcessor {
    /// Process the entire file, calling the provided closure for each line
    fn process_lines<F>(&mut self, line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>;

    /// Skip a specified number of lines from the current position
    fn skip_lines(&mut self, count: u64) -> Result<(), TaleError>;

    /// Get the file size in bytes
    fn file_size(&self) -> u64;

    /// Seek to a specific position in the file
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, ErrReport>;

    /// Get current position in the file
    fn position(&self) -> u64;
}

/// Chonked vs buffered vs can-go-backwards variants.
pub enum FileProcessorType<'a> {
    Buffered(BufferedFileProcessor),
    Chunked(Box<ChunkedFileReader>),
    BackSeeking(BackSeekingProcessor<'a>),
}

impl<'a> FileProcessor for FileProcessorType<'a> {
    fn process_lines<F>(&mut self, line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        match self {
            FileProcessorType::Buffered(processor) => processor.process_lines(line_processor),
            FileProcessorType::Chunked(processor) => processor.process_lines(line_processor),
            FileProcessorType::BackSeeking(processor) => processor.process_lines(line_processor),
        }
    }

    fn skip_lines(&mut self, count: u64) -> Result<(), TaleError> {
        match self {
            FileProcessorType::Buffered(processor) => processor.skip_lines(count),
            FileProcessorType::Chunked(processor) => processor.skip_lines(count),
            FileProcessorType::BackSeeking(processor) => processor.skip_lines(count),
        }
    }

    fn file_size(&self) -> u64 {
        match self {
            FileProcessorType::Buffered(processor) => processor.file_size(),
            FileProcessorType::Chunked(processor) => processor.file_size(),
            FileProcessorType::BackSeeking(processor) => processor.file_size(),
        }
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, ErrReport> {
        match self {
            FileProcessorType::Buffered(processor) => processor.seek(pos),
            FileProcessorType::Chunked(processor) => processor.seek(pos),
            FileProcessorType::BackSeeking(processor) => processor.seek(pos),
        }
    }

    fn position(&self) -> u64 {
        match self {
            FileProcessorType::Buffered(processor) => processor.position(),
            FileProcessorType::Chunked(processor) => processor.position(),
            FileProcessorType::BackSeeking(processor) => processor.position(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::config::ConfigOpts;

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
    fn processor_selection() {
        let fp = PathBuf::from("./fixtures/benchmarks/medium.log");

        // Test 1: Negative offset always uses Simple processor
        crate::config::with_config(
            ConfigOpts {
                offset: -20,
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result = create_file_processor(&fp, Some(1_000_000_000))
                    .expect("should create processor for negative offset");
                assert!(
                    matches!(result, FileProcessorType::BackSeeking(_)),
                    "Negative offset should use Simple processor"
                );
            },
        );

        // Test 2: Byte offset units always use Simple processor
        crate::config::with_config(
            ConfigOpts {
                offset: 100,
                offset_unit: config::OffsetUnit::Bytes,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result =
                    create_file_processor(&fp, Some(1_000_000_000)).expect("should create processor for byte offset");
                assert!(
                    matches!(result, FileProcessorType::BackSeeking(_)),
                    "Byte offset should use Simple processor"
                );
            },
        );

        // Test 3: Block offset units always use Simple processor
        crate::config::with_config(
            ConfigOpts {
                offset: 100,
                offset_unit: config::OffsetUnit::Blocks,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result =
                    create_file_processor(&fp, Some(1_000_000_000)).expect("should create processor for block offset");
                assert!(
                    matches!(result, FileProcessorType::BackSeeking(_)),
                    "Block offset should use Simple processor"
                );
            },
        );

        // Test 4: force_chunked=true uses Chunked processor (when not disabled)
        crate::config::with_config(
            ConfigOpts {
                offset: 100,
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: true,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result =
                    create_file_processor(&fp, Some(100_000_000)).expect("should create processor for force_chunked");
                assert!(
                    matches!(result, FileProcessorType::Chunked(_)),
                    "force_chunked should use Chunked processor"
                );
            },
        );

        // Test 5: disable_chunked=true prevents Chunked processor
        crate::config::with_config(
            ConfigOpts {
                offset: 20_000, // Large offset
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: false,
                disable_chunked: true,
                ..ConfigOpts::default()
            },
            || {
                let result =
                    create_file_processor(&fp, Some(200_000_000)).expect("should create processor for disable_chunked");
                assert!(
                    matches!(result, FileProcessorType::Buffered(_)),
                    "disable_chunked should prevent Chunked processor"
                );
            },
        );

        // Test 6: Large file (>1GB) uses Chunked processor
        crate::config::with_config(
            ConfigOpts {
                offset: 100,
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result = create_file_processor(&fp, Some(1_500_000_000)) // 1.5GB
                    .expect("should create processor for large file");
                assert!(
                    matches!(result, FileProcessorType::Chunked(_)),
                    "Large file (>1GB) should use Chunked processor"
                );
            },
        );

        // Test 7: Large file + large offset uses Chunked processor
        crate::config::with_config(
            ConfigOpts {
                offset: 20_000, // Large offset (>10,000)
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result = create_file_processor(&fp, Some(150_000_000)) // 150MB + large offset
                    .expect("should create processor for large file + large offset");
                assert!(
                    matches!(result, FileProcessorType::Chunked(_)),
                    "Large file (>100MB) + large offset (>10k) should use Chunked processor"
                );
            },
        );

        // Test 8: Small file + small offset uses Buffered processor
        crate::config::with_config(
            ConfigOpts {
                offset: 100, // Small offset
                offset_unit: config::OffsetUnit::Lines,
                force_chunked: false,
                disable_chunked: false,
                ..ConfigOpts::default()
            },
            || {
                let result = create_file_processor(&fp, Some(10_000_000)) // 10MB
                    .expect("should create processor for small file + small offset");
                assert!(
                    matches!(result, FileProcessorType::Buffered(_)),
                    "Small file + small offset should use Buffered processor"
                );
            },
        );
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
    fn test_chunked_file_reader_small_file() -> Result<(), TaleError> {
        let test_data = "line1\nline2\nline3\n";
        let temp_file = create_test_file(test_data);

        let config = ChunkConfig {
            overlap_size: 2,
            low_memory_mode: true,
        };
        let strategy = StaticStrategy {
            chunk_size: 8, // Small chunks to test boundary handling
            config: config.clone(),
        };

        let mut reader = ChunkedFileReader::with_strategy(temp_file.path(), Strategy::Static(strategy))?;

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
    fn buffer_thing_works() -> Result<(), TaleError> {
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
    fn abstract_processor_impl_factory_noun() -> Result<(), TaleError> {
        let test_data = "line1\nline2\nline3\n";
        let temp_file = create_test_file(test_data);

        // Use with_config to isolate this test
        config::with_config(ConfigOpts::default(), || {
            // Small file should use buffered processor
            let mut processor = create_file_processor(temp_file.path(), None).expect("should create processor");
            assert_eq!(processor.file_size(), test_data.len() as u64);

            let mut lines = Vec::new();
            processor
                .process_lines(|line| {
                    lines.push(line.to_string());
                    Ok(())
                })
                .expect("should process lines");

            assert_eq!(lines, vec!["line1", "line2", "line3"]);
        });

        Ok(())
    }

    #[test]
    fn test_chunked_skip_lines() -> Result<(), TaleError> {
        // Create test data with more lines to test chunk boundaries
        let test_data = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n";
        let temp_file = create_test_file(test_data);

        // Use small chunk size to test boundary handling
        let config = ChunkConfig {
            overlap_size: 2,
            low_memory_mode: true,
        };
        let strategy = StaticStrategy {
            chunk_size: 15, // Small enough to split across chunks
            config: config.clone(),
        };

        let mut reader = ChunkedFileReader::with_strategy(temp_file.path(), Strategy::Static(strategy))?;

        // Skip first 3 lines
        reader.skip_lines(3)?;

        // Collect remaining lines
        let mut remaining_lines = Vec::new();
        reader.process_lines(|line| {
            remaining_lines.push(line.to_string());
            Ok(())
        })?;

        // Should have lines 4-10
        assert_eq!(
            remaining_lines,
            vec!["line4", "line5", "line6", "line7", "line8", "line9", "line10"]
        );

        Ok(())
    }

    #[test]
    fn test_chunked_skip_lines_partial_chunk() -> Result<(), TaleError> {
        // Test case where skip_lines needs to stop in the middle of a chunk
        let test_data = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let temp_file = create_test_file(test_data);

        let config = ChunkConfig {
            overlap_size: 1,
            low_memory_mode: true,
        };
        let strategy = StaticStrategy {
            chunk_size: 8, // Will create multiple small chunks
            config: config.clone(),
        };

        let mut reader = ChunkedFileReader::with_strategy(temp_file.path(), Strategy::Static(strategy))?;

        // Skip exactly 5 lines (should stop mid-chunk)
        reader.skip_lines(5)?;

        // Get next line
        let mut next_lines = Vec::new();
        reader.process_lines(|line| {
            next_lines.push(line.to_string());
            if next_lines.len() >= 2 {
                return Ok(()); // Just get first 2 lines after skip
            }
            Ok(())
        })?;

        // Should get lines "f" and "g"
        assert!(next_lines.len() >= 2);
        assert_eq!(next_lines[0], "f");
        assert_eq!(next_lines[1], "g");

        Ok(())
    }
}
