//! A vanilla buffered file reader.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use super::FileProcessor;
use crate::errors::TaleError;

/// Standard buffered file reader implementation
pub struct BufferedFileProcessor {
    reader: BufReader<File>,
    file_size: u64,
    current_position: u64,
}

impl BufferedFileProcessor {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TaleError> {
        let mut file = File::open(&path)?;
        let file_size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;

        let reader = BufReader::new(file);

        Ok(Self {
            reader,
            file_size,
            current_position: 0,
        })
    }
}

impl FileProcessor for BufferedFileProcessor {
    fn process_lines<F>(&mut self, mut line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        let mut line = String::new();
        while self.reader.read_line(&mut line)? > 0 {
            // Remove trailing newline
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }

            line_processor(&line)?;
            line.clear();
        }
        Ok(())
    }

    fn skip_lines(&mut self, count: u64) -> Result<(), TaleError> {
        let mut line = String::new();
        for _ in 0..count {
            if self.reader.read_line(&mut line)? == 0 {
                break; // EOF
            }
            line.clear();
        }
        Ok(())
    }

    fn file_size(&self) -> u64 {
        self.file_size
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, TaleError> {
        let new_pos = self.reader.get_mut().seek(pos).map_err(TaleError::from)?;
        self.current_position = new_pos;
        Ok(new_pos)
    }

    fn position(&self) -> u64 {
        self.current_position
    }
}
