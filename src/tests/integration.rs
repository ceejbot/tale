use std::time::Instant;

use crate::readers::{ChunkedFileReader, FileProcessor};

#[test]
fn test_with_real_log_file() {
    let path = "fixtures/benchmarks/large.log";

    // Time the adaptive version
    let start = Instant::now();
    let mut reader = ChunkedFileReader::new(path).unwrap();
    let mut line_count = 0;
    reader
        .process_lines(|_| {
            line_count += 1;
            Ok(())
        })
        .unwrap();
    let adaptive_time = start.elapsed();

    // Time the static version
    let start = Instant::now();
    let mut reader = ChunkedFileReader::static_optimal(path).unwrap();
    let mut static_line_count = 0;
    reader
        .process_lines(|_| {
            static_line_count += 1;
            Ok(())
        })
        .unwrap();
    let static_time = start.elapsed();

    assert_eq!(line_count, static_line_count);
    println!("Adaptive: {:?}, Static: {:?}", adaptive_time, static_time);

    // Adaptive should be at least as fast
    assert!(adaptive_time * 10 <= static_time * 11); // Allow 10% variance
}
