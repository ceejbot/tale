/// An adaptive chunked reader, which uses its current strategy
/// to decide how to read incoming data. Look, don't complain that I'm
/// overengineering a hobby project. The whole point of a hobby project
/// is learning through overengineering. And why are you reading this
/// comment, anyway?
use crate::errors::TaleError;
use crate::readers::FileProcessor;

/// An adaptive input processor.
pub struct AdaptiveProcessor {
    // TODO
}

/// We must implement this.
impl FileProcessor for AdaptiveProcessor {
    fn process_lines<F>(&mut self, _line_processor: F) -> Result<(), TaleError>
    where
        F: FnMut(&str) -> Result<(), TaleError>,
    {
        todo!()
    }

    fn skip_lines(&mut self, _count: u64) -> Result<(), TaleError> {
        todo!()
    }

    fn file_size(&self) -> u64 {
        todo!()
    }

    fn seek(&mut self, _pos: std::io::SeekFrom) -> Result<u64, miette::Error> {
        todo!()
    }

    fn position(&self) -> u64 {
        todo!()
    }
}
