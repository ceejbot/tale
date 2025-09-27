//! Handle tailing or catting non-multiplex cases while pretty-printing
//! everything we can. We can tuck these simple cases off in its own file and
//! leave them to be simple.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use miette::IntoDiagnostic;

use super::FileProcessor;
use crate::config::OffsetUnit;
use crate::defaults::io::*;
use crate::defaults::processing::*;
use crate::errors::TaleError;
use crate::{config, process_line, strip_line_ending};

pub struct BackSeekingProcessor<'a> {
    fpath: PathBuf,
    initial_file_size: u64,
    file: Option<File>,
    outlock: io::StdoutLock<'a>,
    buffer: BytesMut,
    count: u16,
}

impl<'a> FileProcessor for BackSeekingProcessor<'a> {
    fn process_lines<F>(&mut self, mut line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        // Open file and read all lines, calling processor for each
        let file = File::open(&self.fpath)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            line_processor(&line)?;
        }

        Ok(())
    }

    fn skip_lines(&mut self, count: u64) -> Result<(), TaleError> {
        // Use existing move_to_position logic for line-based offsets
        self.move_to_position(count as i64, OffsetUnit::Lines, false)?;
        Ok(())
    }

    fn file_size(&self) -> u64 {
        self.initial_file_size
    }

    fn seek(&mut self, pos: io::SeekFrom) -> Result<u64, TaleError> {
        if let Some(mut f) = self.file.as_ref() {
            Ok(f.seek(pos).map_err(TaleError::from)?)
        } else {
            let mut file = File::open(&self.fpath).map_err(TaleError::from)?;
            let actual = file.seek(pos).map_err(TaleError::from)?;
            self.file = Some(file);
            Ok(actual)
        }
    }

    fn position(&self) -> u64 {
        if let Some(mut f) = self.file.as_ref() {
            f.stream_position().unwrap_or_default()
        } else {
            0
        }
    }
}

impl<'a> BackSeekingProcessor<'a> {
    pub fn new(fpath: PathBuf) -> Self {
        // briefly open the file and figure out its size
        let file_size = if let Ok(mut file) = File::open(&fpath) {
            file.seek(io::SeekFrom::End(0)).unwrap_or_default()
        } else {
            0
        };

        Self {
            fpath,
            initial_file_size: file_size,
            file: None,
            outlock: io::stdout().lock(),
            buffer: BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY),
            count: 0,
        }
    }

    /// Find the right file offset to start reading & printing this file from,
    /// given the arg input. This seeks forward or backwards by lines, and
    /// returns the current file position. As a side effect, the file is
    /// left at the correct position to begin reading. IMPORTANT: The caller
    /// has to do any last by-lines forward seeking by themselves. This is a
    /// weakness in the internal API.
    pub fn move_to_position(
        &mut self,
        offset: i64,
        units: config::OffsetUnit,
        tailing: bool,
    ) -> Result<File, TaleError> {
        let mut file = File::open(&self.fpath)?;
        // Short circuit if there is no work to do.
        let file_size = file.seek(io::SeekFrom::End(0))?;
        if file_size == 0 {
            return Ok(file);
        }

        // Reset to start after size read.
        file.seek(io::SeekFrom::Start(0))?;

        // Set our position in the file based on offset unit.
        match units {
            config::OffsetUnit::Lines => {
                if offset > 0 {
                    // Positive offset: skip N lines from the beginning,
                    // which we do NOT do here
                    file.seek(io::SeekFrom::Start(0))?;
                } else if offset < 0 {
                    // Negative offset: start N lines from the end
                    let start = self.move_n_lines_back(&mut file, (-offset) as u64)?;
                    file.seek(io::SeekFrom::Start(start))?;
                } else if tailing {
                    // Zero offset: start from the end (no lines to show unless tailing)
                    file.seek(io::SeekFrom::End(0))?;
                }
            }
            config::OffsetUnit::Bytes => {
                // Byte-based offset
                if offset > 0 {
                    // Positive offset: skip N bytes from the beginning
                    file.seek(io::SeekFrom::Start(offset as u64))?;
                } else if offset < 0 {
                    // Negative offset: start N bytes from the end
                    file.seek(io::SeekFrom::End(offset))?;
                } else if tailing {
                    // Zero offset: start from the end
                    file.seek(io::SeekFrom::End(0))?;
                }
            }
            config::OffsetUnit::Blocks => {
                // This case is the as above, but we multiply offset by block size.
                if offset > 0 {
                    let byte_offset = (offset as u64) * BLOCK_SIZE;
                    file.seek(io::SeekFrom::Start(byte_offset))?;
                } else if offset < 0 {
                    let byte_offset = offset * (BLOCK_SIZE as i64);
                    file.seek(io::SeekFrom::End(byte_offset))?;
                } else if tailing {
                    file.seek(io::SeekFrom::End(0))?;
                }
            }
        }

        Ok(file)
    }

    /// Find the byte offset from the beginning of the file for the start of the
    /// line to begin our pretty-printing. This is the seek backwards version.
    /// It is made entirely of edge cases. Used only by
    /// FileProcessor::move_to_position().
    fn move_n_lines_back(&mut self, file: &mut File, line_count: u64) -> Result<u64, TaleError> {
        let file_size = file.seek(io::SeekFrom::End(0))?;
        if file_size == 0 {
            return Ok(0);
        }

        const BUFFER_SIZE: usize = 8192;
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut lines_found = 0u64;

        // First check if the file ends with a newline
        file.seek(io::SeekFrom::End(-1))?;
        let mut last_byte = [0u8; 1];
        file.read_exact(&mut last_byte)?;
        let ends_with_newline = last_byte[0] == b'\n';

        // To get the last N lines, we need to find the right number of newlines
        // For a file that doesn't end with newline: last line is after the last newline
        // For a file that ends with newline: last line is between the last two newlines
        let target_newlines = if ends_with_newline { line_count } else { line_count - 1 };

        let mut pos = file_size;

        loop {
            // how much should we read?
            let chunk_size = std::cmp::min(BUFFER_SIZE as u64, pos) as usize;
            if chunk_size == 0 {
                // We've reached the beginning of the file
                return Ok(0);
            }

            // Read a chonk. Chunk. Whatever.
            pos -= chunk_size as u64;
            file.seek(io::SeekFrom::Start(pos))?;
            file.read_exact(&mut buffer[..chunk_size])?;

            // Count newlines in reverse order
            for (i, &byte) in buffer[..chunk_size].iter().enumerate().rev() {
                if byte == b'\n' {
                    lines_found += 1;
                    if lines_found > target_newlines {
                        // Found enough lines, return position after this newline
                        return Ok(pos + i as u64 + 1);
                    }
                }
            }

            // We hit the beginning: not enough lines. We start at the very
            // beginning, a very good place to start.
            if pos == 0 {
                return Ok(0);
            }
        }
    }

    /// Process a single line through the formatting pipeline
    pub fn process_line(&mut self, line: &str) -> Result<(), TaleError> {
        process_line(line, &mut self.buffer, &mut self.outlock)
            .map_err(|e| TaleError::from(std::io::Error::other(e.to_string())))?;
        self.count += 1;
        self.flush_if_needed()
    }

    /// Flush output if we've processed enough lines
    pub fn flush_if_needed(&mut self) -> Result<(), TaleError> {
        if self.count >= FLUSH_LINE_COUNT {
            self.outlock.flush()?;
            self.count = 0;
        }
        Ok(())
    }

    /// Force flush output
    pub fn flush(&mut self) -> Result<(), TaleError> {
        self.outlock.flush()?;
        self.count = 0;
        Ok(())
    }

    pub fn tail(&mut self) -> miette::Result<()> {
        let tailing = config::tailing();
        let offset_unit = config::offset_unit();
        let offset = config::offset();

        let file = self
            .move_to_position(offset, offset_unit, tailing)
            .map_err(miette::Report::from)?;
        let mut reader = BufReader::new(file);

        // If we've got a positive line offset, we still need to skip our N lines
        if offset > 0 && matches!(offset_unit, config::OffsetUnit::Lines) {
            let consume_me = (&mut reader).lines().take(offset as usize);
            // We then must consume them. this feels v inefficient but I do not know.
            let _count = consume_me.count();
        };

        // Now at last we get to start printing. What a fuss.
        let mut line = String::with_capacity(LINE_CAPACITY);
        while reader.read_line(&mut line).into_diagnostic()? != 0 {
            strip_line_ending(&mut line);
            self.process_line(line.as_str()).map_err(miette::Report::from)?;
            line.clear();
        }
        self.flush().map_err(miette::Report::from)?;

        if !tailing {
            return Ok(());
        }

        // Now we tell a tale of tailing.
        let mut last_flush = Instant::now();

        // Get the file back from the reader
        let mut file = reader.into_inner();
        let mut file_position = file.stream_position().into_diagnostic()?;

        // Polling loop for file growth detection
        // Note: Could be enhanced with inotify/file system events for efficiency
        loop {
            std::thread::sleep(Duration::from_millis(100));

            // Check if file has grown
            let current_size = file.seek(io::SeekFrom::End(0)).into_diagnostic()?;
            if current_size > file_position {
                // Hide and seek, trains and sewing machines.
                file.seek(io::SeekFrom::Start(file_position)).into_diagnostic()?;
                let mut tail_reader = BufReader::new(&file);

                match tail_reader.read_line(&mut line).into_diagnostic()? {
                    0 => {
                        // EOF - no new data available, continue polling
                        continue;
                    }
                    _ => {
                        strip_line_ending(&mut line);
                        // New data available - process it.
                        process_line(&line, &mut self.buffer, &mut self.outlock)?;
                        if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                            self.outlock.flush().into_diagnostic()?;
                            last_flush = Instant::now();
                        }

                        line.clear();
                        self.buffer.clear();
                    }
                }

                // Note where we finished reading so we can figure out if we get more.
                file_position = file.stream_position().into_diagnostic()?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeking_backwards() {
        use std::io::{Read, Seek, Write};

        use tempfile::NamedTempFile;

        // Create a temporary file with known content
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let content = "line1\nline2\nline3\nline4\nline5\n";
        temp_file
            .write_all(content.as_bytes())
            .expect("Failed to write to temp file");

        let pathbuf = PathBuf::from(temp_file.path());
        let mut processor = BackSeekingProcessor::new(pathbuf);
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        // Test getting last 2 lines (should start after "line3\n")
        let pos = processor
            .move_n_lines_back(&mut file, 2)
            .expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line4\nline5\n");

        // Test getting last line (should start after "line4\n")
        let pos = processor
            .move_n_lines_back(&mut file, 1)
            .expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line5\n");

        // Test getting more lines than exist (should start from beginning)
        let pos = processor
            .move_n_lines_back(&mut file, 10)
            .expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn seeking_in_empty() {
        use tempfile::NamedTempFile;
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let pathbuf = PathBuf::from(temp_file.path());
        let mut processor = BackSeekingProcessor::new(pathbuf);
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        let pos = processor
            .move_n_lines_back(&mut file, 5)
            .expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn good_circular_buffer_byte_logic() {
        // Test the circular buffer logic without stdin dependency
        let input_data = b"0123456789abcdefghij";
        let buffer_size = 10;
        let mut circular_buffer = vec![0u8; buffer_size];
        let mut pos = 0usize;

        // Simulate writing to circular buffer
        for &byte in input_data {
            circular_buffer[pos % buffer_size] = byte;
            pos += 1;
        }

        // Should have wrapped around, last 10 bytes should be "abcdefghij"
        let _total_read = input_data.len() as u64;
        let _bytes_to_show = buffer_size as u64;
        let start_pos = pos % buffer_size;

        // Extract the last bytes_to_show bytes
        let mut result = Vec::with_capacity(buffer_size);
        for i in 0..buffer_size {
            result.push(circular_buffer[(start_pos + i) % buffer_size]);
        }

        assert_eq!(result, b"abcdefghij");
    }

    #[test]
    fn circular_buffer_partial_fill_works() {
        // Test circular buffer when input is smaller than buffer
        let input_data = b"hello";
        let buffer_size = 10;
        let mut circular_buffer = vec![0u8; buffer_size];

        // Fill buffer
        for (i, &byte) in input_data.iter().enumerate() {
            circular_buffer[i] = byte;
        }

        let bytes_to_output = input_data.len();
        let result = &circular_buffer[..bytes_to_output];

        assert_eq!(result, b"hello");
    }

    #[test]
    fn circular_line_logic() {
        use std::collections::VecDeque;

        // Test VecDeque circular behavior for line buffering
        let lines_to_keep = 3;
        let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(lines_to_keep);

        let input_lines = vec!["line1", "line2", "line3", "line4", "line5"];

        for line in input_lines {
            if line_buffer.len() >= lines_to_keep {
                line_buffer.pop_front();
            }
            line_buffer.push_back(line.to_string());
        }

        // Should contain last 3 lines
        let result: Vec<String> = line_buffer.into_iter().collect();
        assert_eq!(result, vec!["line3", "line4", "line5"]);
    }

    #[test]
    fn handles_overshoots_correctly() {
        let overshoot = b"partial line\ncomplete line\nanother";
        let overshoot_str = String::from_utf8_lossy(overshoot);
        let mut remaining = overshoot_str.as_ref();
        let mut complete_lines = Vec::new();

        while let Some(pos) = remaining.find('\n') {
            complete_lines.push(&remaining[..pos]);
            remaining = &remaining[pos + 1..];
        }

        assert_eq!(complete_lines, vec!["partial line", "complete line"]);
        assert_eq!(remaining, "another");
    }
}
