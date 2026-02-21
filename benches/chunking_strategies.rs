//! Benchmark for chunk sizing and file processing
//!
//! Tests performance of:
//! - Chunk size calculation for different file sizes
//! - End-to-end file processing with optimal chunk sizing

use std::hint::black_box;
use std::io::Write;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tale_ndjson::config::ConfigOpts;
use tale_ndjson::readers::strategies::StaticStrategy;
use tale_ndjson::readers::{ChunkedFileReader, FileProcessor};
use tempfile::NamedTempFile;

/// Generate test data of specified size
fn generate_test_data(lines: usize) -> Vec<u8> {
    let mut data = Vec::new();
    for i in 0..lines {
        let line = format!(
            r#"{{"timestamp":"2024-01-01T10:{:02}:{:02}Z","level":"INFO","message":"Test message {}","request_id":"req-{}"}}"#,
            (i / 60) % 60,
            i % 60,
            i,
            i
        );
        data.extend_from_slice(line.as_bytes());
        data.push(b'\n');
    }
    data
}

/// Create a temporary file with test data
fn create_test_file(data: &[u8]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(data).expect("Failed to write test data");
    file.flush().expect("Failed to flush test data");
    file
}

/// Benchmark chunk size calculation for different file sizes
fn bench_chunk_size_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_size_calculation");

    let file_sizes: Vec<(&str, u64)> = vec![
        ("small", 1024),              // 1KB
        ("medium", 1024 * 1024),      // 1MB
        ("large", 100 * 1024 * 1024), // 100MB
    ];

    for (name, size) in file_sizes {
        group.bench_with_input(BenchmarkId::new("optimal_for_file", name), &size, |b, &size| {
            b.iter(|| black_box(StaticStrategy::optimal_for_file(size)))
        });
    }

    group.finish();
}

/// Benchmark end-to-end file processing
fn bench_file_processing(c: &mut Criterion) {
    // Initialize config for the library
    tale_ndjson::config::set(ConfigOpts::default()).expect("Failed to initialize config");

    let mut group = c.benchmark_group("file_processing");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(15));

    let test_data = generate_test_data(1000); // 1000 lines

    group.bench_function("process_1k_lines", |b| {
        b.iter(|| {
            let temp_file = create_test_file(&test_data);
            let mut reader = ChunkedFileReader::new(temp_file.path()).expect("Failed to create reader");

            let mut line_count = 0;
            reader
                .process_lines(|_line| {
                    line_count += 1;
                    Ok(())
                })
                .expect("Failed to process lines");

            black_box(line_count)
        })
    });

    group.finish();
}

criterion_group!(benches, bench_chunk_size_calculation, bench_file_processing);

criterion_main!(benches);
