use std::time::Instant;

use crate::readers::{ChunkedFileReader, FileProcessor};

#[test]
fn use_real_log_file() {
    let temp_file = super::create_large_test_file(1_000_000); // 1M lines

    let start = Instant::now();
    let mut reader = ChunkedFileReader::new(&temp_file).expect("test fixtures must exist");
    let mut line_count = 0;
    reader
        .process_lines(|_| {
            line_count += 1;
            Ok(())
        })
        .expect("process_lines should succeed");
    let elapsed = start.elapsed();

    assert_eq!(line_count, 1_000_000);
    println!("Processed 1M lines in {:?}", elapsed);
}

#[test]
fn throughput_is_reasonable() {
    let temp_file = super::create_large_test_file(1_000_000); // 1M lines

    let start = Instant::now();
    let mut reader = ChunkedFileReader::static_optimal(&temp_file).expect("test fixture should exist");

    let mut total_bytes = 0;
    while let Some(chunk) = reader.read_chunk().expect("failed reading chunk!") {
        total_bytes += chunk.size();
    }
    let elapsed = start.elapsed();
    let throughput = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();

    eprintln!(
        "Throughput: {:.2} MB/s ({} bytes in {:?})",
        throughput, total_bytes, elapsed
    );

    // Should process at a reasonable speed (>10 MB/s even on slow systems)
    assert!(throughput > 10.0, "Throughput too low: {:.2} MB/s", throughput);
}
