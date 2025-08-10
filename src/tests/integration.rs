use std::time::Instant;

use crate::readers::{AdaptiveStrategy, ChunkedFileReader, FileProcessor, StaticStrategy, Strategy};

#[test]
fn test_with_real_log_file() {
    let temp_file = super::create_large_test_file(1_000_000); // 1M lines

    // Time the adaptive version
    let start = Instant::now();
    let mut reader = ChunkedFileReader::new(&temp_file).expect("test fixtures must exist");
    let mut line_count = 0;
    reader
        .process_lines(|_| {
            line_count += 1;
            Ok(())
        })
        .unwrap();
    let adaptive_time = start.elapsed();

    // Measure with static
    let start = Instant::now();
    let mut reader = ChunkedFileReader::static_optimal(&temp_file).unwrap();
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
    // FAILS:
    //     Adaptive: 1.915423459s, Static: 1.725438084s
    assert!(adaptive_time * 10 <= static_time * 11); // Allow 10% variance
}

#[test]
fn test_throughput_improves() {
    let temp_file = super::create_large_test_file(1_000_000); // 1M lines

    // Measure with adaptive
    let start = Instant::now();
    let mut adaptive_reader =
        ChunkedFileReader::with_strategy(&temp_file, Strategy::Adaptive(AdaptiveStrategy::default()))
            .expect("chunked file reader creation should succeed");

    let mut total_bytes = 0;
    while let Some(chunk) = adaptive_reader.read_chunk().expect("failed reading chunk!") {
        total_bytes += chunk.size();
    }
    let adaptive_time = start.elapsed();
    let adaptive_throughput = (total_bytes as f64 / (1024.0 * 1024.0)) / adaptive_time.as_secs_f64();

    eprintln!("Adaptive throughput: {:.2} MB/s", adaptive_throughput);

    let start = Instant::now();
    let mut static_reader = ChunkedFileReader::with_strategy(&temp_file, Strategy::Static(StaticStrategy::default()))
        .expect("chunked file reader creation should succeed");

    let mut total_bytes = 0;
    while let Some(chunk) = static_reader.read_chunk().expect("failed reading chunk!") {
        total_bytes += chunk.size();
    }
    let static_time = start.elapsed();
    let static_throughput = (total_bytes as f64 / (1024.0 * 1024.0)) / static_time.as_secs_f64();

    eprintln!("Static throughput: {:.2} MB/s", static_throughput);

    // Adaptive should be at least as good
    assert!(adaptive_throughput >= static_throughput * 0.95);
}
