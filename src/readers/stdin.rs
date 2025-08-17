//! Handle stdin with offsets. We never multiplex stdin, so this is a simpler
//! case than many others in some ways, and a more challenging one in others.
//! Specifically, scrolling *back* by negative offsets can be tricky when the
//! offsets are large.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use miette::WrapErr;

use crate::defaults::io::*;
use crate::defaults::memory::*;
use crate::defaults::processing::BLOCK_SIZE;
use crate::errors::TaleError;
use crate::{config, process_line, strip_line_ending};

/// Entry point for processing stdin all the ways we need to handle it.
pub fn handle_stdin() -> Result<(), TaleError> {
    let offset = config::offset();
    let offset_unit = config::offset_unit();

    let mut processor = StdinProcessor::new();

    if offset == 0 {
        return processor.tail();
    }

    match (offset.is_positive(), offset_unit) {
        // Positive offsets: skip first N units
        (true, config::OffsetUnit::Lines) => processor.skip_lines(offset as u64),
        (true, config::OffsetUnit::Bytes) => processor.skip_bytes(offset as u64),
        (true, config::OffsetUnit::Blocks) => {
            let bytes_to_skip = (offset as u64) * BLOCK_SIZE;
            processor.skip_bytes(bytes_to_skip)
        }

        // Negative offsets: show last N units
        (false, config::OffsetUnit::Lines) => processor.backtrack_lines((-offset) as u64),
        (false, config::OffsetUnit::Bytes) => processor.backtrack_bytes((-offset) as u64),
        (false, config::OffsetUnit::Blocks) => processor.backtrack_bytes(((-offset) as u64) * BLOCK_SIZE),
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

impl<'a> Default for StdinProcessor<'a> {
    fn default() -> Self {
        Self::new()
    }
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
    pub fn process_line(&mut self, line: &str) -> Result<(), TaleError> {
        process_line(line, &mut self.buffer, &mut self.outlock).with_context(|| "Failed to process line")?;
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

    /// Read a line from stdin, stripping line endings
    pub fn read_line(&mut self) -> Result<usize, TaleError> {
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
    pub fn process_to_end(&mut self) -> Result<(), TaleError> {
        while self.read_line()? != 0 {
            let line = self.line().to_string();
            self.process_line(&line)?;
        }
        self.flush()
    }

    /// We have a partial buffer left over from a read. Seek back,
    /// then continue processing.
    pub fn handle_overshoot(&mut self, overshoot: &[u8]) -> Result<(), TaleError> {
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
            let line = self.line().to_string();
            self.process_line(&line)?;
        }

        // Now process the rest normally
        self.tail()
    }

    /// Enter normal processing mode - process input until EOF, then poll for
    /// more
    pub fn tail(&mut self) -> Result<(), TaleError> {
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
                    let line = self.line().to_string();
                    self.process_line(&line)?;
                    if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                        self.flush()?;
                        last_flush = Instant::now();
                    }
                }
            }
        }
    }

    pub fn skip_lines(&mut self, count: u64) -> Result<(), TaleError> {
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
    pub fn skip_bytes(&mut self, to_skip: u64) -> Result<(), TaleError> {
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

    pub fn backtrack_bytes(&mut self, bytes_to_show: u64) -> Result<(), TaleError> {
        let mut circular_buffer = CircularByteBuffer::new(bytes_to_show as usize);

        // Read all input into circular buffer
        loop {
            let bytes_read = self.inlock.read(&mut self.buffer)?;
            if bytes_read == 0 {
                break; // EOF
            }
            circular_buffer.write(&self.buffer[..bytes_read]);
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

    /// Show last N lines from stdin (adaptive approach with circular buffer)
    pub fn backtrack_lines(&mut self, lines_to_show: u64) -> Result<(), TaleError> {
        use std::collections::VecDeque;

        use tempfile::NamedTempFile;

        let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(lines_to_show as usize);
        let mut memory_used = 0usize;
        let mut temp_file: Option<NamedTempFile> = None;

        // Read all input, keeping only the last N lines
        loop {
            let bytes_read = self.read_line()?;
            if bytes_read == 0 {
                break; // EOF
            }

            // Check if we need to switch to temp file mode
            if memory_used > MEMORY_LIMIT_BYTES && temp_file.is_none() {
                // Create temp file and write current buffer to it
                let mut temp = NamedTempFile::new().map_err(|e| {
                    TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                        operation: "create temporary file for large stdin backtrack".to_string(),
                        path: None,
                        source: e,
                    }))
                })?;

                // Write existing buffer to temp file
                for line in &line_buffer {
                    writeln!(temp, "{}", line).map_err(|e| {
                        TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                            operation: "write to temporary file".to_string(),
                            path: None,
                            source: e,
                        }))
                    })?;
                }

                temp_file = Some(temp);

                // Clear memory buffer since we're now using temp file
                line_buffer.clear();
                memory_used = 0;
            }

            match &mut temp_file {
                Some(temp) => {
                    // Write to temp file
                    writeln!(temp, "{}", self.line).map_err(|e| {
                        TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                            operation: "write to temporary file".to_string(),
                            path: None,
                            source: e,
                        }))
                    })?;
                }
                None => {
                    // Add to circular buffer in memory
                    if line_buffer.len() >= lines_to_show as usize {
                        // Remove oldest line and update memory usage
                        if let Some(old_line) = line_buffer.pop_front() {
                            memory_used -= old_line.len();
                        }
                    }
                    memory_used += self.line.len();
                    line_buffer.push_back(self.line.clone());
                }
            }
        }

        // Output the result
        match temp_file {
            Some(mut temp) => {
                // Flush and read back last N lines from temp file
                temp.flush().map_err(|e| {
                    TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                        operation: "flush temporary file".to_string(),
                        path: None,
                        source: e,
                    }))
                })?;
                self.read_last_n_lines_from_temp_file(temp, lines_to_show)?;
            }
            None => {
                // Output the buffered lines from memory
                for buffered_line in line_buffer {
                    self.process_line(&buffered_line)?;
                }
            }
        }

        self.flush()?;
        Ok(())
    }

    /// Read the last N lines from a temporary file
    fn read_last_n_lines_from_temp_file(
        &mut self,
        temp_file: tempfile::NamedTempFile,
        lines_to_show: u64,
    ) -> Result<(), TaleError> {
        use std::collections::VecDeque;
        use std::fs::File;

        // Reopen the temp file for reading
        let file = File::open(temp_file.path()).map_err(|e| {
            TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                operation: "open temporary file for reading".to_string(),
                path: Some(temp_file.path().to_path_buf()),
                source: e,
            }))
        })?;
        let reader = BufReader::new(file);

        let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(lines_to_show as usize);

        // Read all lines, keeping only the last N
        for line_result in reader.lines() {
            let line = line_result.map_err(|e| {
                TaleError::from(Box::new(crate::errors::IoError::OperationFailed {
                    operation: "read line from temporary file".to_string(),
                    path: None,
                    source: e,
                }))
            })?;

            if line_buffer.len() >= lines_to_show as usize {
                line_buffer.pop_front();
            }
            line_buffer.push_back(line);
        }

        // Output the last N lines
        for line in line_buffer {
            self.process_line(&line)?;
        }

        // Temp file will be automatically deleted when NamedTempFile is dropped
        Ok(())
    }
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

fn find_last_char(buffer: &[u8], c: u8) -> Option<usize> {
    buffer.iter().rposition(|&b| b == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_find_last_char() {
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
    fn backtracking_with_partial_lines() {
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
}
