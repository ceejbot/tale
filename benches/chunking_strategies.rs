//! Benchmark comparing different chunking strategies
//! 
//! This benchmark tests the performance of:
//! - StaticStrategy vs AdaptiveStrategy vs ConservativeStrategy
//! - Different chunk sizes and file sizes
//! - Strategy adaptation behavior over time

use std::io::Write;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use tempfile::NamedTempFile;

use tale_ndjson::config::ConfigOpts;
use tale_ndjson::readers::strategies::{Strategy, IsStrategy, AdaptiveStrategy, StaticStrategy, ConservativeStrategy};
use tale_ndjson::readers::{ChunkedFileReader, FileProcessor};
use tale_ndjson::metrics::ChunkMetrics;

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

/// Benchmark strategy selection for different file sizes
fn bench_strategy_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_selection");
    
    let file_sizes = vec![
        ("small", 1024),      // 1KB
        ("medium", 1024 * 1024), // 1MB  
        ("large", 100 * 1024 * 1024), // 100MB
    ];
    
    for (name, size) in file_sizes {
        group.bench_with_input(BenchmarkId::new("from_config", name), &size, |b, &size| {
            b.iter(|| {
                let config = ConfigOpts::default();
                black_box(Strategy::from_config(&config, Some(size)))
            })
        });
    }
    
    group.finish();
}

/// Benchmark initial chunk size calculation
fn bench_initial_chunk_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("initial_chunk_size");
    
    let strategies = vec![
        ("static", Strategy::Static(StaticStrategy::default())),
        ("adaptive", Strategy::Adaptive(AdaptiveStrategy::default())),
        ("conservative", Strategy::Conservative(ConservativeStrategy::default())),
    ];
    
    for (name, strategy) in strategies {
        group.bench_with_input(BenchmarkId::new("calculate", name), &strategy, |b, strategy| {
            b.iter(|| black_box(strategy.initial_chunk_size()))
        });
    }
    
    group.finish();
}

/// Benchmark strategy adaptation with different performance scenarios
fn bench_strategy_adaptation(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_adaptation");
    group.measurement_time(Duration::from_secs(10));
    
    // Create metrics for different scenarios
    let fast_metrics = {
        let mut metrics = ChunkMetrics::new();
        metrics.record_chunk_processing(32 * 1024, Duration::from_millis(5), 150);
        metrics.record_chunk_processing(32 * 1024, Duration::from_millis(4), 160);
        metrics.record_chunk_processing(32 * 1024, Duration::from_millis(3), 170);
        metrics
    };
    
    let slow_metrics = {
        let mut metrics = ChunkMetrics::new();
        metrics.record_chunk_processing(128 * 1024, Duration::from_millis(50), 80);
        metrics.record_chunk_processing(128 * 1024, Duration::from_millis(60), 75);
        metrics.record_chunk_processing(128 * 1024, Duration::from_millis(70), 70);
        metrics
    };
    
    let scenarios = vec![
        ("fast_performance", &fast_metrics),
        ("slow_performance", &slow_metrics),
    ];
    
    for (scenario_name, metrics) in scenarios {
        let adaptive = AdaptiveStrategy::default();
        let current_size = adaptive.initial_chunk_size();
        
        group.bench_with_input(
            BenchmarkId::new("adaptive_adapt", scenario_name), 
            metrics, 
            |b, metrics| {
                b.iter(|| {
                    let mut strategy = adaptive.clone();
                    black_box(strategy.adapt_size(metrics, current_size))
                })
            }
        );
    }
    
    group.finish();
}

/// Benchmark end-to-end file processing with different strategies
fn bench_file_processing_strategies(c: &mut Criterion) {
    // Initialize config for the library
    tale_ndjson::config::set(ConfigOpts::default())
        .expect("Failed to initialize config");
    
    let mut group = c.benchmark_group("file_processing");
    group.sample_size(20); // Fewer samples for file I/O benchmarks
    group.measurement_time(Duration::from_secs(15));
    
    // Generate test data (moderate size for reasonable benchmark time)
    let test_data = generate_test_data(1000); // 1000 lines
    
    let strategies = vec![
        ("static", Strategy::Static(StaticStrategy::default())),
        ("adaptive", Strategy::Adaptive(AdaptiveStrategy::default())),
        ("conservative", Strategy::Conservative(ConservativeStrategy::default())),
    ];
    
    for (name, strategy) in strategies {
        group.bench_with_input(BenchmarkId::new("process_file", name), &strategy, |b, _strategy| {
            b.iter(|| {
                let temp_file = create_test_file(&test_data);
                let mut reader = ChunkedFileReader::new(temp_file.path())
                    .expect("Failed to create reader");
                
                let mut line_count = 0;
                reader.process_lines(|_line| {
                    line_count += 1;
                    Ok(())
                }).expect("Failed to process lines");
                
                black_box(line_count)
            })
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_strategy_selection,
    bench_initial_chunk_size,
    bench_strategy_adaptation,
    bench_file_processing_strategies
);

criterion_main!(benches);
