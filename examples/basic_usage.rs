//! Basic usage example for tale-ndjson library
//!
//! This example demonstrates how to use the tale library to process
//! NDJSON log files with different reader strategies.

use std::io::Write;
use std::path::Path;

use tale_ndjson::readers::{BufferedFileProcessor, ChunkedFileReader};
use tale_ndjson::{FileProcessor, TaleError};
use tempfile::NamedTempFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Tale NDJSON Processing Example");

    // Create a sample NDJSON log file
    let sample_data = vec![
        r#"{"timestamp":"2024-01-01T10:00:00Z","level":"info","message":"Application started","request_id":"req-123"}"#,
        r#"{"timestamp":"2024-01-01T10:01:00Z","level":"warn","message":"High memory usage detected","memory_mb":512}"#,
        r#"{"timestamp":"2024-01-01T10:02:00Z","level":"error","message":"Database connection failed","error":"timeout after 5s"}"#,
        r#"{"timestamp":"2024-01-01T10:03:00Z","level":"info","message":"Request completed","method":"GET","url":"/api/users","status":200,"elapsed":125}"#,
    ];

    // Create temporary file
    let mut temp_file = NamedTempFile::new()?;
    for line in &sample_data {
        writeln!(temp_file, "{}", line)?;
    }
    let temp_path = temp_file.path();

    println!("\n--- Processing with BufferedFileProcessor ---");
    process_with_buffered_reader(temp_path)?;

    println!("\n--- Processing with ChunkedFileReader ---");
    process_with_chunked_reader(temp_path)?;

    Ok(())
}

fn process_with_buffered_reader(path: &Path) -> Result<(), TaleError> {
    let mut reader = BufferedFileProcessor::new(path)?;
    let mut line_count = 0;

    reader.process_lines(|line| {
        line_count += 1;
        println!("Line {}: {}", line_count, line);
        Ok(())
    })?;

    println!("Processed {} lines with BufferedFileProcessor", line_count);
    Ok(())
}

fn process_with_chunked_reader(path: &Path) -> Result<(), TaleError> {
    let mut reader = ChunkedFileReader::new(path)?;
    let mut line_count = 0;

    reader.process_lines(|line| {
        line_count += 1;
        println!("Line {}: {}", line_count, line);
        Ok(())
    })?;

    println!("Processed {} lines with ChunkedFileReader", line_count);
    Ok(())
}
