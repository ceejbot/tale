//! Handle tailing or catting non-multiplex cases while pretty-printing
//! everything we can. We can tuck these simple cases off in its own file and
//! leave them to be simple.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bytes::{Buf, BytesMut};

use crate::constants::*;
use crate::{config, process_line, strip_line_ending};

pub fn handle_stdin() -> anyhow::Result<()> {
    // Pre-allocate based on typical log line length (~200-300 chars)
    let mut line = String::with_capacity(512);
    let mut buffer = BytesMut::with_capacity(1024);
    let mut inlock = io::stdin().lock();
    let mut outlock = io::stdout().lock();

    let tail = config::tailing();

    let mut count: u16 = 0;

    // Process initial input until EOF
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

    // We flush at intervals when tailing, because we might wait a
    // long time for more data.
    let mut last_flush = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(100));

        match inlock.read_line(&mut line)? {
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
            }
        }
    }
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

    // Set our position in the file based on offset unit.
    match offset_unit {
        config::Offset::Lines => {
            // Traditional line-based offset (existing behavior)
            if offset > 0 {
                // Positive offset: skip N lines from the beginning
                file.seek(io::SeekFrom::Start(0))?;
            } else if offset < 0 {
                // Negative offset: start N lines from the end
                let start = seek_backwards(&mut file, (-offset) as u64)?;
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
            if offset > 0 {
                // Positive offset: skip N blocks from the beginning
                let byte_offset = (offset as u64) * BLOCK_SIZE;
                file.seek(io::SeekFrom::Start(byte_offset))?;
            } else if offset < 0 {
                // Negative offset: start N blocks from the end
                let byte_offset = (offset as i64) * (BLOCK_SIZE as i64);
                file.seek(io::SeekFrom::End(byte_offset))?;
            } else if tailing {
                // Zero offset: start from the end
                file.seek(io::SeekFrom::End(0))?;
            }
        }
    }

    let mut reader = BufReader::new(file);

    // If we've got a positive line offset, we still need to skip our N lines
    if offset > 0 && matches!(offset_unit, config::Offset::Lines) {
        let consume_me = (&mut reader).lines().take(offset as usize);
        // We then must consume them. this feels v inefficient but I do not know.
        let _count = consume_me.count();
    };

    // Now at last we get to start printing. What a fuss.
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);
    let mut line = String::with_capacity(1024);

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

/// Find the byte offset from the beginning of the file for the start of the
/// line to begin our pretty-printing. This is the seek backwards version. It is
/// made entirely of edge cases.
fn seek_backwards(file: &mut File, lines_from_end: u64) -> anyhow::Result<u64> {
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
    let target_newlines = if ends_with_newline {
        lines_from_end
    } else {
        lines_from_end - 1
    };

    let mut pos = file_size;

    loop {
        // Calculate how much to read in this chunk
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
        let pos = seek_backwards(&mut file, 2).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line4\nline5\n");

        // Test getting last line (should start after "line4\n")
        let pos = seek_backwards(&mut file, 1).expect("Failed to find position");
        file.seek(io::SeekFrom::Start(pos)).expect("Failed to seek");

        let mut remaining = String::new();
        file.read_to_string(&mut remaining).expect("Failed to read remaining");
        assert_eq!(remaining, "line5\n");

        // Test getting more lines than exist (should start from beginning)
        let pos = seek_backwards(&mut file, 10).expect("Failed to find position");
        assert_eq!(pos, 0);
    }

    #[test]
    fn seeking_in_empty() {
        use tempfile::NamedTempFile;
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut file = File::open(temp_file.path()).expect("Failed to open temp file");

        let pos = seek_backwards(&mut file, 5).expect("Failed to find position");
        assert_eq!(pos, 0);
    }
}
