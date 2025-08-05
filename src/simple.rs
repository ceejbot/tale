//! Handle tailing or catting non-multiplex cases while pretty-printing
//! everything we can. We can tuck these simple cases off in its own file and
//! leave them to be simple.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use bytes::BytesMut;

use crate::constants::*;
use crate::{config, process_line, strip_line_ending};

/// Entry point for handling a file.
pub fn handle_file(fpath: &PathBuf) -> anyhow::Result<()> {
    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }

    let mut processor = FileProcessor::new(fpath.to_path_buf());
    // here, tail handles offsets
    processor.tail()
}

/// Entry point for processing stdin all the ways we need to handle it.
pub fn handle_stdin() -> anyhow::Result<()> {
    let offset = config::offset();
    let offset_unit = config::offset_unit();

    let mut processor = StdinProcessor::new();

    if offset == 0 {
        return processor.tail();
    }

    match (offset.is_positive(), offset_unit) {
        // Positive offsets: skip first N units
        (true, config::Offset::Lines) => processor.skip_lines(offset as u64),
        (true, config::Offset::Bytes) => processor.skip_bytes(offset as u64),
        (true, config::Offset::Blocks) => {
            let bytes_to_skip = (offset as u64) * BLOCK_SIZE;
            processor.skip_bytes(bytes_to_skip)
        }

        // Negative offsets: show last N units
        (false, config::Offset::Lines) => processor.backtrack_lines((-offset) as u64),
        (false, config::Offset::Bytes) => processor.backtrack_bytes((-offset) as u64),
        (false, config::Offset::Blocks) => processor.backtrack_bytes(((-offset) as u64) * BLOCK_SIZE),
    }
}

/// Handles common stdin processing patterns with automatic flushing and tailing
/// support
pub struct StdinProcessor<'a> {
    inlock: io::StdinLock<'a>,
    outlock: io::StdoutLock<'a>,
    buffer: BytesMut,
    line: String,
    count: u16,
}

impl<'a> StdinProcessor<'a> {
    /// Create a new StdinProcessor with standard buffer sizes
    pub fn new() -> Self {
        Self {
            inlock: io::stdin().lock(),
            outlock: io::stdout().lock(),
            buffer: BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY),
            line: String::with_capacity(LINE_CAPACITY),
            count: 0,
        }
    }

    /// Process a single line through the formatting pipeline
    pub fn process_line(&mut self, line: &str) -> anyhow::Result<()> {
        process_line(line, &mut self.buffer, &mut self.outlock).with_context(|| "Failed to process line")?;
        self.count += 1;
        self.flush_if_needed()
    }

    /// Flush output if we've processed enough lines
    pub fn flush_if_needed(&mut self) -> anyhow::Result<()> {
        if self.count >= FLUSH_LINE_COUNT {
            self.outlock.flush().context("Failed to flush stdout")?;
            self.count = 0;
        }
        Ok(())
    }

    /// Force flush output
    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.outlock.flush().context("Failed to flush stdout")?;
        self.count = 0;
        Ok(())
    }

    /// Read a line from stdin, stripping line endings
    pub fn read_line(&mut self) -> anyhow::Result<usize> {
        self.line.clear();
        let bytes_read = self.inlock.read_line(&mut self.line)?;
        if bytes_read > 0 {
            strip_line_ending(&mut self.line);
        }
        Ok(bytes_read)
    }

    /// Get the current line content
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Process all remaining input until EOF
    pub fn process_to_end(&mut self) -> anyhow::Result<()> {
        while self.read_line()? != 0 {
            self.process_line(&self.line().to_string())?;
        }
        self.flush()
    }

    /// We have a partial buffer left over from a read. Seek back,
    /// then continue processing.
    pub fn handle_overshoot(&mut self, overshoot: &[u8]) -> anyhow::Result<()> {
        // Process any complete lines in the overshoot buffer using byte operations
        let mut start = 0;
        for (i, &byte) in overshoot.iter().enumerate() {
            if byte == b'\n' {
                // Found a complete line
                let line_bytes = &overshoot[start..i];
                let line = String::from_utf8_lossy(line_bytes);
                self.process_line(&line)?;
                start = i + 1;
            }
        }

        // If there's a partial line remaining, add it to our line buffer
        if start < overshoot.len() {
            let remaining_bytes = &overshoot[start..];
            let remaining_str = String::from_utf8_lossy(remaining_bytes);
            self.line.push_str(&remaining_str);
        }

        // Read the rest of the partial line (if any)
        if !self.line.is_empty() && self.inlock.read_line(&mut self.line)? > 0 {
            strip_line_ending(&mut self.line);
            self.process_line(&self.line().to_string())?;
        }

        // Now process the rest normally
        self.tail()
    }

    /// Enter normal processing mode - process input until EOF, then poll for
    /// more
    pub fn tail(&mut self) -> anyhow::Result<()> {
        self.process_to_end()?;
        if !config::tailing() {
            return Ok(());
        }

        let mut last_flush = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(100));

            match self.read_line()? {
                0 => continue, // EOF - keep polling
                _ => {
                    self.process_line(&self.line().to_string())?;
                    if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                        self.flush()?;
                        last_flush = Instant::now();
                    }
                }
            }
        }
    }

    pub fn skip_lines(&mut self, count: u64) -> anyhow::Result<()> {
        // Skip the requested number of lines
        let mut lines_skipped = 0u64;
        while lines_skipped < count {
            match self.read_line()? {
                0 => {
                    // EOF reached before skipping enough lines - nothing to output
                    return Ok(());
                }
                _ => {
                    lines_skipped += 1;
                }
            }
        }
        self.tail()
    }

    // skip bytes then keep going, tailing if config says to tail
    pub fn skip_bytes(&mut self, to_skip: u64) -> anyhow::Result<()> {
        let mut buffer = [0u8; READ_BUFFER_SIZE];
        let mut bytes_skipped = 0u64;

        while bytes_skipped < to_skip {
            let bytes_read = self.inlock.read(&mut buffer)?;
            if bytes_read == 0 {
                // EOF reached before skipping enough bytes - nothing to output
                return Ok(());
            }

            let bytes_to_consume = std::cmp::min(bytes_read as u64, to_skip - bytes_skipped);
            bytes_skipped += bytes_to_consume;

            // If we read more than we needed to skip, we need to handle the overshoot
            if bytes_skipped == to_skip && bytes_to_consume < bytes_read as u64 {
                let overshoot_start = bytes_to_consume as usize;
                let overshoot = &buffer[overshoot_start..bytes_read];
                return self.handle_overshoot(overshoot);
            }
        }

        // Process remaining input normally (no overshoot)
        self.tail()
    }

    pub fn backtrack_bytes(&mut self, bytes_to_show: u64) -> anyhow::Result<()> {
        let mut circular_buffer = CircularByteBuffer::new(bytes_to_show as usize);

        // Read all input into circular buffer
        loop {
            let bytes_read = self.inlock.read(&mut self.buffer)?;
            if bytes_read == 0 {
                break; // EOF
            }
            circular_buffer.write(&mut self.buffer[..bytes_read]);
        }

        // Check if we have any data
        if circular_buffer.is_empty() {
            return Ok(()); // No input
        }

        let mut overshoot: Vec<u8> = Vec::new();
        let output_bytes = circular_buffer.extract_last_bytes();
        let process_this = match find_last_char(output_bytes.as_slice(), b'\n') {
            Some(last_line_ending) => {
                overshoot = output_bytes[last_line_ending + 1..].to_vec();
                output_bytes[..last_line_ending].to_vec()
            }
            None => output_bytes[..].to_vec(),
        };

        // Process the output bytes line by line
        let output_str = String::from_utf8_lossy(&process_this);
        for line in output_str.lines() {
            self.process_line(line)?;
        }
        // if we have anything left over, we need to handle the overshoot
        if !overshoot.is_empty() {
            self.handle_overshoot(overshoot.as_slice())
        } else {
            self.tail()
        }
    }

    /// Get the current line as a mutable reference
    pub fn line_mut(&mut self) -> &mut String {
        &mut self.line
    }

    /// Show last N lines from stdin (adaptive approach with circular buffer)
    pub fn backtrack_lines(&mut self, lines_to_show: u64) -> anyhow::Result<()> {
        use std::collections::VecDeque;

        let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(lines_to_show as usize);
        let mut memory_used = 0usize;

        // Read all input, keeping only the last N lines
        loop {
            let bytes_read = self.read_line()?;
            if bytes_read == 0 {
                break; // EOF
            }

            // Add to circular buffer
            if line_buffer.len() >= lines_to_show as usize {
                // Remove oldest line and update memory usage
                if let Some(old_line) = line_buffer.pop_front() {
                    memory_used -= old_line.len();
                }
            }

            memory_used += self.line.len();
            line_buffer.push_back(self.line.clone());

            // Check memory limit - if exceeded, we would need temp file fallback
            // For now, just warn and continue (temp file implementation would go here)
            if memory_used > MEMORY_LIMIT_BYTES {
                eprintln!(
                    "Warning: Memory limit exceeded for line buffering. Consider using byte/block offsets for large inputs."
                );
                // TODO implement temp files feature here
            }
        }

        // Output the buffered lines
        for buffered_line in line_buffer {
            self.process_line(&buffered_line)?;
        }
        self.flush()?;

        Ok(())
    }
}

fn find_last_char(buffer: &[u8], c: u8) -> Option<usize> {
    buffer.iter().rposition(|&b| b == c)
}

/// Circular buffer for efficiently storing and retrieving the last N bytes
pub struct CircularByteBuffer {
    buffer: Vec<u8>,
    pos: usize,
    total_read: u64,
    capacity: usize,
}

impl CircularByteBuffer {
    /// Create a new circular buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0u8; capacity],
            pos: 0,
            total_read: 0,
            capacity,
        }
    }

    /// Write data to the circular buffer
    pub fn write(&mut self, data: &[u8]) {
        for &byte in data {
            self.buffer[self.pos % self.capacity] = byte;
            self.pos += 1;
            self.total_read += 1;
        }
    }

    /// Extract the last N bytes from the buffer (up to capacity)
    pub fn extract_last_bytes(&self) -> Vec<u8> {
        if self.total_read == 0 {
            return Vec::new();
        }

        let bytes_to_output = std::cmp::min(self.total_read, self.capacity as u64) as usize;

        if self.total_read >= self.capacity as u64 {
            // Full circular buffer case - need to wrap around
            let start_pos = self.pos % self.capacity;
            let mut result = Vec::with_capacity(bytes_to_output);
            for i in 0..bytes_to_output {
                result.push(self.buffer[(start_pos + i) % self.capacity]);
            }
            result
        } else {
            // Partial buffer case - just take what we have
            self.buffer[..bytes_to_output].to_vec()
        }
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.total_read == 0
    }

    /// Get the total number of bytes that have been written
    pub fn total_written(&self) -> u64 {
        self.total_read
    }
}

pub struct FileProcessor<'a> {
    fpath: PathBuf,
    outlock: io::StdoutLock<'a>,
    buffer: BytesMut,
    count: u16,
}

impl<'a> FileProcessor<'a> {
    pub fn new(fpath: PathBuf) -> Self {
        Self {
            fpath,
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
    pub fn move_to_position(&mut self, offset: i64, units: config::Offset, tailing: bool) -> anyhow::Result<File> {
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
            config::Offset::Lines => {
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
            config::Offset::Bytes => {
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
            config::Offset::Blocks => {
                // This case is the as above, but we multiply offset by block size.
                if offset > 0 {
                    let byte_offset = (offset as u64) * BLOCK_SIZE;
                    file.seek(io::SeekFrom::Start(byte_offset))?;
                } else if offset < 0 {
                    let byte_offset = (offset as i64) * (BLOCK_SIZE as i64);
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
    fn move_n_lines_back(&mut self, file: &mut File, line_count: u64) -> anyhow::Result<u64> {
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
    pub fn process_line(&mut self, line: &str) -> anyhow::Result<()> {
        process_line(line, &mut self.buffer, &mut self.outlock).with_context(|| "Failed to process line")?;
        self.count += 1;
        self.flush_if_needed()
    }

    /// Flush output if we've processed enough lines
    pub fn flush_if_needed(&mut self) -> anyhow::Result<()> {
        if self.count >= FLUSH_LINE_COUNT {
            self.outlock.flush().context("Failed to flush stdout")?;
            self.count = 0;
        }
        Ok(())
    }

    /// Force flush output
    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.outlock.flush().context("Failed to flush stdout")?;
        self.count = 0;
        Ok(())
    }

    pub fn tail(&mut self) -> anyhow::Result<()> {
        let tailing = config::tailing();
        let offset_unit = config::offset_unit();
        let offset = config::offset();

        let file = self.move_to_position(offset, offset_unit, tailing)?;
        let mut reader = BufReader::new(file);

        // If we've got a positive line offset, we still need to skip our N lines
        if offset > 0 && matches!(offset_unit, config::Offset::Lines) {
            let consume_me = (&mut reader).lines().take(offset as usize);
            // We then must consume them. this feels v inefficient but I do not know.
            let _count = consume_me.count();
        };

        // Now at last we get to start printing. What a fuss.
        let mut line = String::with_capacity(LINE_CAPACITY);
        while reader.read_line(&mut line)? != 0 {
            strip_line_ending(&mut line);
            self.process_line(line.as_str())?;
            line.clear();
        }
        self.flush()?;

        if !tailing {
            return Ok(());
        }

        // Now we tell a tale of tailing.
        let mut last_flush = Instant::now();

        // Get the file back from the reader
        let mut file = reader.into_inner();
        let mut file_position = file.stream_position()?;

        // polling loop. TODO consider better impl
        loop {
            std::thread::sleep(Duration::from_millis(100));

            // Check if file has grown
            let current_size = file.seek(io::SeekFrom::End(0))?;
            if current_size > file_position {
                // Hide and seek, trains and sewing machines.
                file.seek(io::SeekFrom::Start(file_position))?;
                let mut tail_reader = BufReader::new(&file);

                match tail_reader.read_line(&mut line)? {
                    0 => {
                        // EOF - no new data available, continue polling
                        continue;
                    }
                    _ => {
                        strip_line_ending(&mut line);
                        // New data available - process it.
                        process_line(&line, &mut self.buffer, &mut self.outlock)?;
                        if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                            self.outlock.flush()?;
                            last_flush = Instant::now();
                        }

                        line.clear();
                        self.buffer.clear();
                    }
                }

                // Note where we finished reading so we can figure out if we get more.
                file_position = file.stream_position()?;
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
        let mut processor = FileProcessor::new(pathbuf);
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
        let mut processor = FileProcessor::new(pathbuf);
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        let pos = processor
            .move_n_lines_back(&mut file, 5)
            .expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn test_circular_buffer_byte_logic() {
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
    fn test_circular_buffer_partial_fill() {
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
    fn test_find_last_char() {
        // Test normal case
        assert_eq!(find_last_char(b"hello\nworld\n", b'\n'), Some(11));
        assert_eq!(find_last_char(b"hello\nworld", b'\n'), Some(5));

        // Test no match
        assert_eq!(find_last_char(b"hello world", b'\n'), None);

        // Test empty buffer
        assert_eq!(find_last_char(b"", b'\n'), None);

        // Test single character
        assert_eq!(find_last_char(b"\n", b'\n'), Some(0));
        assert_eq!(find_last_char(b"a", b'\n'), None);

        // Test multiple occurrences
        assert_eq!(find_last_char(b"\n\n\n", b'\n'), Some(2));
    }

    #[test]
    fn circular_buffer_edge_cases() {
        // Test buffer exactly at capacity
        let mut buffer = CircularByteBuffer::new(5);
        buffer.write(b"12345");
        assert_eq!(buffer.extract_last_bytes(), b"12345");

        // Test buffer overflow
        buffer.write(b"67890");
        assert_eq!(buffer.extract_last_bytes(), b"67890");

        // Test partial writes
        let mut buffer2 = CircularByteBuffer::new(10);
        buffer2.write(b"abc");
        assert_eq!(buffer2.extract_last_bytes(), b"abc");

        // Test multiple small writes
        buffer2.write(b"def");
        buffer2.write(b"ghi");
        assert_eq!(buffer2.extract_last_bytes(), b"abcdefghi");
    }

    #[test]
    fn test_backtrack_bytes_partial_line_handling() {
        // Test that backtrack_bytes properly handles partial lines at the end
        // This tests the logic without actual I/O

        // Case 1: Buffer ends with newline
        let buffer_with_newline = b"line1\nline2\nline3\n";
        assert_eq!(find_last_char(buffer_with_newline, b'\n'), Some(17));

        // Case 2: Buffer doesn't end with newline
        let buffer_without_newline = b"line1\nline2\nline3";
        assert_eq!(find_last_char(buffer_without_newline, b'\n'), Some(11));

        // Case 3: Buffer with no newlines
        let buffer_no_newlines = b"single long line without newlines";
        assert_eq!(find_last_char(buffer_no_newlines, b'\n'), None);
    }

    #[test]
    fn test_overshoot_handling_logic() {
        // Test the overshoot parsing logic
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
