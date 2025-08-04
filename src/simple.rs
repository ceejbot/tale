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
        // Convert initial bytes to string and process any complete lines
        let initial_str = String::from_utf8_lossy(overshoot);
        let mut remaining = initial_str.as_ref();

        // Process any complete lines in the initial bytes
        // I don't like this _at all_. Should be using byte ops.
        while let Some(pos) = remaining.find('\n') {
            let complete_line = &remaining[..pos];
            self.process_line(complete_line)?;
            remaining = &remaining[pos + 1..];
        }

        // If there's a partial line remaining, add it to our line buffer
        if !remaining.is_empty() {
            self.line.push_str(remaining);
        }

        // Now process the rest normally
        // TODO replace this func
        // process_remaining_stdin(inlock, line)
        self.tail()
    }

    /// Enter normal processing mode - process input until EOF, then poll for more
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

    pub fn skip_bytes(&mut self, to_skip: u64) -> anyhow::Result<()> {
        // skip bytes then keep going, tailing if config says to tail

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

    /// Get mutable access to stdin lock for advanced operations
    pub fn stdin_lock(&mut self) -> &mut io::StdinLock<'a> {
        &mut self.inlock
    }

    /// Get the current line as a mutable reference
    pub fn line_mut(&mut self) -> &mut String {
        &mut self.line
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

pub fn handle_stdin() -> anyhow::Result<()> {
    let offset = config::offset();
    let offset_unit = config::offset_unit();

    let mut processor = StdinProcessor::new();

    // Original no-offset implementation
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
        (false, config::Offset::Lines) => process_negative_line_offset((-offset) as u64),
        (false, config::Offset::Bytes) => process_negative_byte_offset((-offset) as u64),
        (false, config::Offset::Blocks) => process_negative_block_offset((-offset) as u64),
    }
}

/// Skip first N bytes, then process normally
fn process_positive_byte_offset(bytes_to_skip: u64) -> anyhow::Result<()> {
    let mut inlock = io::stdin().lock();
    let mut buffer = [0u8; READ_BUFFER_SIZE];
    let mut bytes_skipped = 0u64;

    // Skip the requested number of bytes
    while bytes_skipped < bytes_to_skip {
        let bytes_read = inlock.read(&mut buffer)?;
        if bytes_read == 0 {
            // EOF reached before skipping enough bytes - nothing to output
            return Ok(());
        }

        let bytes_to_consume = std::cmp::min(bytes_read as u64, bytes_to_skip - bytes_skipped);
        bytes_skipped += bytes_to_consume;

        // If we read more than we needed to skip, we need to handle the overshoot
        if bytes_skipped == bytes_to_skip && bytes_to_consume < bytes_read as u64 {
            let overshoot_start = bytes_to_consume as usize;
            let overshoot = &buffer[overshoot_start..bytes_read];

            // Process the overshoot data line by line
            return process_remaining_stdin_with_initial_bytes(inlock, overshoot);
        }
    }

    // Process remaining input normally (no overshoot)
    let line = String::with_capacity(LINE_CAPACITY);
    process_remaining_stdin(inlock, line)
}

/// Process remaining stdin input normally (helper function)
fn process_remaining_stdin(mut inlock: io::StdinLock<'_>, mut line: String) -> anyhow::Result<()> {
    let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);
    let mut outlock = io::stdout().lock();
    let tail = config::tailing();
    let mut count: u16 = 0;

    // Process remaining input until EOF
    while inlock.read_line(&mut line)? != 0 {
        strip_line_ending(&mut line);
        process_line(&line, &mut buffer, &mut outlock)?;
        count += 1;
        if count >= FLUSH_LINE_COUNT {
            outlock.flush()?;
            count = 0;
        }
        line.clear();
    }
    outlock.flush()?;

    if !tail {
        return Ok(());
    }

    // Tail mode: continue polling for new input
    let mut last_flush = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(100));

        match inlock.read_line(&mut line)? {
            0 => continue, // EOF - keep polling
            _ => {
                strip_line_ending(&mut line);
                process_line(&line, &mut buffer, &mut outlock)?;
                if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                    outlock.flush()?;
                    last_flush = Instant::now();
                }
                line.clear();
            }
        }
    }
}

/// Process remaining stdin with some initial bytes from byte offset overshoot
fn process_remaining_stdin_with_initial_bytes(inlock: io::StdinLock<'_>, initial_bytes: &[u8]) -> anyhow::Result<()> {
    let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);
    let mut outlock = io::stdout().lock();
    let mut line = String::with_capacity(LINE_CAPACITY);
    let mut count: u16 = 0;

    // Convert initial bytes to string and process any complete lines
    let initial_str = String::from_utf8_lossy(initial_bytes);
    let mut remaining = initial_str.as_ref();

    // Process any complete lines in the initial bytes
    while let Some(newline_pos) = remaining.find('\n') {
        let complete_line = &remaining[..newline_pos];
        process_line(complete_line, &mut buffer, &mut outlock)?;
        count += 1;
        if count >= FLUSH_LINE_COUNT {
            outlock.flush()?;
            count = 0;
        }
        remaining = &remaining[newline_pos + 1..];
    }

    // If there's a partial line remaining, add it to our line buffer
    if !remaining.is_empty() {
        line.push_str(remaining);
    }

    // Now process the rest normally
    process_remaining_stdin(inlock, line)
}

/// Show last N bytes from stdin (circular buffer approach)
fn process_negative_byte_offset(bytes_to_show: u64) -> anyhow::Result<()> {
    let mut inlock = io::stdin().lock();
    let mut circular_buffer = CircularByteBuffer::new(bytes_to_show as usize);
    let mut buffer = [0u8; READ_BUFFER_SIZE];

    // Read all input into circular buffer
    loop {
        let bytes_read = inlock.read(&mut buffer)?;
        if bytes_read == 0 {
            break; // EOF
        }
        circular_buffer.write(&buffer[..bytes_read]);
    }

    // Check if we have any data
    if circular_buffer.is_empty() {
        return Ok(()); // No input
    }

    // Output from circular buffer
    let mut outlock = io::stdout().lock();
    let mut output_buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);

    // Extract and process the last bytes
    let output_bytes = circular_buffer.extract_last_bytes();

    // Process the output bytes line by line
    let output_str = String::from_utf8_lossy(&output_bytes);
    for line in output_str.lines() {
        process_line(line, &mut output_buffer, &mut outlock)?;
    }
    outlock.flush()?;

    Ok(())
}

/// Show last N blocks (N * 512 bytes) from stdin
fn process_negative_block_offset(blocks_to_show: u64) -> anyhow::Result<()> {
    let bytes_to_show = blocks_to_show * BLOCK_SIZE;
    process_negative_byte_offset(bytes_to_show)
}

/// Show last N lines from stdin (adaptive approach with circular buffer)
fn process_negative_line_offset(lines_to_show: u64) -> anyhow::Result<()> {
    use std::collections::VecDeque;

    let mut inlock = io::stdin().lock();
    let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(lines_to_show as usize);
    let mut line = String::with_capacity(LINE_CAPACITY);
    let mut memory_used = 0usize;

    // Read all input, keeping only the last N lines
    loop {
        line.clear();
        let bytes_read = inlock.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        strip_line_ending(&mut line);

        // Add to circular buffer
        if line_buffer.len() >= lines_to_show as usize {
            // Remove oldest line and update memory usage
            if let Some(old_line) = line_buffer.pop_front() {
                memory_used -= old_line.len();
            }
        }

        memory_used += line.len();
        line_buffer.push_back(line.clone());

        // Check memory limit - if exceeded, we would need temp file fallback
        // For now, just warn and continue (temp file implementation would go here)
        if memory_used > MEMORY_LIMIT_BYTES {
            eprintln!(
                "Warning: Memory limit exceeded for line buffering. Consider using byte/block offsets for large inputs."
            );
            // TODO implement temp files feature here
        }
    }

    // splort out our buffered lines
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);

    for buffered_line in line_buffer {
        process_line(&buffered_line, &mut buffer, &mut outlock)?;
    }
    outlock.flush()?;

    Ok(())
}

pub fn handle_file(fpath: &PathBuf) -> anyhow::Result<()> {
    use std::io::Seek;

    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }

    let tailing = config::tailing();
    let offset_unit = config::offset_unit();
    let offset = config::offset();
    let mut file = File::open(&fpath)?;

    let _unused = move_to_position(&mut file, offset, offset_unit, tailing)?;
    let mut reader = BufReader::new(file);

    // If we've got a positive line offset, we still need to skip our N lines
    if offset > 0 && matches!(offset_unit, config::Offset::Lines) {
        let consume_me = (&mut reader).lines().take(offset as usize);
        // We then must consume them. this feels v inefficient but I do not know.
        let _count = consume_me.count();
    };

    // Now at last we get to start printing. What a fuss.
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(OUTPUT_BUFFER_CAPACITY);
    let mut line = String::with_capacity(LINE_CAPACITY);

    let mut count: u16 = 0;
    while reader.read_line(&mut line)? != 0 {
        strip_line_ending(&mut line);
        process_line(&line, &mut buffer, &mut outlock)?;
        count += 1;
        if count >= FLUSH_LINE_COUNT {
            outlock.flush()?;
            count = 0;
        }
        line.clear();
    }
    outlock.flush()?;

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
                    process_line(&line, &mut buffer, &mut outlock)?;
                    if last_flush.elapsed() >= TAIL_FLUSH_INTERVAL {
                        outlock.flush()?;
                        last_flush = Instant::now();
                    }

                    line.clear();
                    buffer.clear();
                }
            }

            // Note where we finished reading so we can figure out if we get more.
            file_position = file.stream_position()?;
        }
    }
}

/// Find the right file offset to start reading & printing this file from, given
/// the arg input. This seeks forward or backwards by lines, and returns the
/// current file position. As a side effect, the file is left at the correct
/// position to begin reading. IMPORTANT: The caller has to do any last by-lines
/// forward seeking by themselves. This is a weakness in the internal API.
pub fn move_to_position(file: &mut File, offset: i64, units: config::Offset, tailing: bool) -> anyhow::Result<u64> {
    // Short circuit if there is no work to do.
    let file_size = file.seek(io::SeekFrom::End(0))?;
    if file_size == 0 {
        return Ok(0);
    }

    // Set our position in the file based on offset unit.
    match units {
        config::Offset::Lines => {
            if offset > 0 {
                // Positive offset: skip N lines from the beginning,
                // which we do NOT do here
                file.seek(io::SeekFrom::Start(0))?;
            } else if offset < 0 {
                // Negative offset: start N lines from the end
                let start = move_n_lines_back(file, (-offset) as u64)?;
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

    // Unused now, but just in case. Might refactor away.
    Ok(file.stream_position()?)
}

/// Find the byte offset from the beginning of the file for the start of the
/// line to begin our pretty-printing. This is the seek backwards version. It is
/// made entirely of edge cases.
fn move_n_lines_back(file: &mut File, line_count: u64) -> anyhow::Result<u64> {
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

        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        // Test getting last 2 lines (should start after "line3\n")
        let pos = move_n_lines_back(&mut file, 2).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line4\nline5\n");

        // Test getting last line (should start after "line4\n")
        let pos = move_n_lines_back(&mut file, 1).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line5\n");

        // Test getting more lines than exist (should start from beginning)
        let pos = move_n_lines_back(&mut file, 10).expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn seeking_in_empty() {
        use tempfile::NamedTempFile;
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        let pos = move_n_lines_back(&mut file, 5).expect("Failed to find position");
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
    fn test_line_buffer_circular_logic() {
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
    fn test_memory_usage_calculation() {
        use std::collections::VecDeque;

        // Test memory usage tracking for line buffer
        let mut line_buffer: VecDeque<String> = VecDeque::new();
        let mut memory_used = 0usize;

        let test_lines = vec!["short", "a bit longer line", "tiny"];

        for line in &test_lines {
            memory_used += line.len();
            line_buffer.push_back(line.to_string());
        }

        let expected_memory = test_lines.iter().map(|s| s.len()).sum::<usize>();
        assert_eq!(memory_used, expected_memory);

        // Test memory usage after removing a line
        if let Some(removed) = line_buffer.pop_front() {
            memory_used -= removed.len();
        }

        let remaining_memory = test_lines[1..].iter().map(|s| s.len()).sum::<usize>();
        assert_eq!(memory_used, remaining_memory);
    }
}
