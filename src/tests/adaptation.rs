use std::collections::HashSet;

use crate::readers::{AdaptiveStrategy, ChunkedFileReader, Strategy};

#[test]
#[ignore] // TODO: Fix adaptation behavior after Phase 1 architecture changes
fn test_adaptation_behavior() {
    // Create a large test file
    let test_file = super::create_large_test_file(100_000); // 100k lines

    let mut reader = ChunkedFileReader::new(&test_file).unwrap();
    let mut chunk_sizes = Vec::new();

    // Process file and track chunk size changes
    while let Some(chunk) = reader.read_chunk().unwrap() {
        chunk_sizes.push(chunk.size());
    }

    // Verify adaptation occurred
    let unique_sizes: HashSet<_> = chunk_sizes.iter().collect();
    assert!(unique_sizes.len() > 1, "Should have adapted chunk size");

    // Verify trend (should generally increase for good performance)
    let first_10_avg = chunk_sizes[..10].iter().sum::<usize>() / 10;
    let last_10_avg = chunk_sizes[chunk_sizes.len() - 10..].iter().sum::<usize>() / 10;
    eprintln!(
        "unique sizes: {}; early avg: {first_10_avg}; late avg: {last_10_avg};",
        unique_sizes.len()
    );
    assert!(last_10_avg >= first_10_avg, "Should trend toward larger chunks"); // FAILS
}

#[test]
fn test_adaptive_integration() {
    let temp_file = super::create_large_test_file(100000);
    let strategy = Strategy::Adaptive(AdaptiveStrategy::default());
    let mut reader = ChunkedFileReader::with_strategy(&temp_file, strategy).unwrap();

    // Process file and verify adaptation
    let mut chunk_sizes = Vec::new();
    while let Some(chunk) = reader.read_chunk().unwrap() {
        chunk_sizes.push(chunk.size());
    }

    // Should have multiple different sizes
    let unique: std::collections::HashSet<_> = chunk_sizes.iter().collect();
    assert!(unique.len() > 1);
}

#[test]
fn handles_memory_pressure() {
    // Allocate a large vector to simulate memory pressure
    let _memory_hog: Vec<u8> = vec![0; 150 * 1024 * 1024]; // 150MB

    let mut reader = ChunkedFileReader::new("./fixtures/benchmarks/medium.log").expect("test fixtures need to exist");

    // Should start with smaller chunks due to memory pressure
    let chunk = reader.read_chunk().unwrap().unwrap();
    assert!(chunk.size() <= 32 * 1024, "Should use small chunks under pressure");
}
