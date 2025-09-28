//! File processing performance benchmarks
//! 
//! Compares performance of different file processors:
//! - BufferedFileProcessor vs ChunkedFileReader vs BackSeekingProcessor
//! - Different file sizes and processing patterns
//! - Real-world JSON parsing performance

use std::io::Write;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use tempfile::NamedTempFile;

use tale_ndjson::config::ConfigOpts;
use tale_ndjson::readers::{BufferedFileProcessor, ChunkedFileReader, BackSeekingProcessor, FileProcessor};

/// Generate realistic NDJSON test data
fn generate_ndjson_data(lines: usize) -> Vec<u8> {
    let mut data = Vec::new();
    
    let log_levels = ["DEBUG", "INFO", "WARN", "ERROR"];
    let components = ["auth", "api", "database", "cache", "worker"];
    let messages = [
        "Request processed successfully",
        "Database connection established", 
        "Cache miss for key",
        "Authentication token validated",
        "Background job completed",
        "Rate limit exceeded for user",
        "Memory usage above threshold",
        "SSL handshake completed",
    ];
    
    for i in 0..lines {
        let level = log_levels[i % log_levels.len()];
        let component = components[i % components.len()];
        let message = messages[i % messages.len()];
        
        let line = if i % 10 == 0 {
            // Every 10th line: Rich structured log (canonical format)
            format!(
                r#"{{"timestamp":"2024-01-01T{:02}:{:02}:{:02}Z","level":"{}","message":"{}","method":"GET","url":"/api/v1/users/{}","status":"200","elapsed":"{}","size":"{}","request_id":"req-{}","remote_host":"192.168.1.{}","user_agent":"test-client/1.0"}}"#,
                (i / 3600) % 24, (i / 60) % 60, i % 60,
                level, message, i % 1000,
                25 + (i % 200),
                1024 + (i % 4096),
                i,
                1 + (i % 254)
            )
        } else if i % 5 == 0 {
            // Every 5th line: Medium structured log
            format!(
                r#"{{"timestamp":"2024-01-01T{:02}:{:02}:{:02}Z","level":"{}","message":"{}","component":"{}","request_id":"req-{}","user_id":"{}","elapsed_ms":"{}"}}"#,
                (i / 3600) % 24, (i / 60) % 60, i % 60,
                level, message, component, i, i % 10000, 10 + (i % 100)
            )
        } else {
            // Simple structured log
            format!(
                r#"{{"timestamp":"2024-01-01T{:02}:{:02}:{:02}Z","level":"{}","message":"{}","request_id":"req-{}"}}"#,
                (i / 3600) % 24, (i / 60) % 60, i % 60,
                level, message, i
            )
        };
        
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

/// Benchmark different file processors on various file sizes
fn bench_file_processors(c: &mut Criterion) {
    // Initialize config for the library
    tale_ndjson::config::set(ConfigOpts::default())
        .expect("Failed to initialize config");
    
    let mut group = c.benchmark_group("file_processors");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));
    
    let file_sizes = vec![
        ("small", 100),    // 100 lines (~10KB)
        ("medium", 1000),  // 1000 lines (~100KB)
        ("large", 5000),   // 5000 lines (~500KB) 
    ];
    
    for (size_name, lines) in file_sizes {
        let test_data = generate_ndjson_data(lines);
        
        // BufferedFileProcessor
        group.bench_with_input(
            BenchmarkId::new("buffered", size_name),
            &test_data,
            |b, data| {
                b.iter(|| {
                    let temp_file = create_test_file(data);
                    let mut reader = BufferedFileProcessor::new(temp_file.path())
                        .expect("Failed to create reader");
                    
                    let mut line_count = 0;
                    reader.process_lines(|_line| {
                        line_count += 1;
                        Ok(())
                    }).expect("Failed to process lines");
                    
                    black_box(line_count)
                })
            }
        );
        
        // ChunkedFileReader
        group.bench_with_input(
            BenchmarkId::new("chunked", size_name),
            &test_data,
            |b, data| {
                b.iter(|| {
                    let temp_file = create_test_file(data);
                    let mut reader = ChunkedFileReader::new(temp_file.path())
                        .expect("Failed to create reader");
                    
                    let mut line_count = 0;
                    reader.process_lines(|_line| {
                        line_count += 1;
                        Ok(())
                    }).expect("Failed to process lines");
                    
                    black_box(line_count)
                })
            }
        );
        
        // BackSeekingProcessor
        group.bench_with_input(
            BenchmarkId::new("backseeking", size_name),
            &test_data,
            |b, data| {
                b.iter(|| {
                    let temp_file = create_test_file(data);
                    let mut reader = BackSeekingProcessor::new(temp_file.path().to_path_buf());
                    
                    let mut line_count = 0;
                    reader.process_lines(|_line| {
                        line_count += 1;
                        Ok(())
                    }).expect("Failed to process lines");
                    
                    black_box(line_count)
                })
            }
        );
    }
    
    group.finish();
}

/// Benchmark skip operations for different processors
fn bench_skip_operations(c: &mut Criterion) {
    // Initialize config for the library
    tale_ndjson::config::set(ConfigOpts::default())
        .expect("Failed to initialize config");
        
    let mut group = c.benchmark_group("skip_operations");
    group.sample_size(30);
    
    let test_data = generate_ndjson_data(1000); // 1000 lines
    let skip_counts = vec![10, 100, 500];
    
    for skip_count in skip_counts {
        // ChunkedFileReader skip
        group.bench_with_input(
            BenchmarkId::new("chunked_skip", skip_count),
            &(test_data.clone(), skip_count),
            |b, (data, skip)| {
                b.iter(|| {
                    let temp_file = create_test_file(data);
                    let mut reader = ChunkedFileReader::new(temp_file.path())
                        .expect("Failed to create reader");
                    
                    reader.skip_lines(*skip as u64)
                        .expect("Failed to skip lines");
                    
                    let mut line_count = 0;
                    reader.process_lines(|_line| {
                        line_count += 1;
                        Ok(())
                    }).expect("Failed to process lines");
                    
                    black_box(line_count)
                })
            }
        );
        
        // BackSeekingProcessor skip
        group.bench_with_input(
            BenchmarkId::new("backseeking_skip", skip_count),
            &(test_data.clone(), skip_count),
            |b, (data, skip)| {
                b.iter(|| {
                    let temp_file = create_test_file(data);
                    let mut reader = BackSeekingProcessor::new(temp_file.path().to_path_buf());
                    
                    reader.skip_lines(*skip as u64)
                        .expect("Failed to skip lines");
                    
                    let mut line_count = 0;
                    reader.process_lines(|_line| {
                        line_count += 1;
                        Ok(())
                    }).expect("Failed to process lines");
                    
                    black_box(line_count)
                })
            }
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_file_processors,
    bench_skip_operations
);

criterion_main!(benches);
