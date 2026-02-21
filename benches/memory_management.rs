//! Memory management benchmarks
//!
//! Tests performance of:
//! - Memory budget allocation and tracking
//! - Memory pressure detection
//! - Memory-constrained strategy adaptation

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tale_ndjson::MemoryBudget;
use tale_ndjson::config::ConfigOpts;
use tale_ndjson::metrics::detect_memory_pressure;
use tale_ndjson::readers::strategies::{ConservativeStrategy, IsStrategy};

/// Benchmark memory budget creation
fn bench_memory_budget_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_budget_creation");

    let sizes = vec![
        ("small", 10 * 1024 * 1024),   // 10MB
        ("medium", 100 * 1024 * 1024), // 100MB
        ("large", 1024 * 1024 * 1024), // 1GB
    ];

    for (name, size) in sizes {
        group.bench_with_input(BenchmarkId::new("new_budget", name), &size, |b, &size| {
            b.iter(|| black_box(MemoryBudget::new(size).expect("Failed to create budget")))
        });
    }

    group.finish();
}

/// Benchmark memory allocation and deallocation
fn bench_memory_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_allocation");

    // Create a memory budget for testing
    let budget = MemoryBudget::new(100 * 1024 * 1024).expect("Failed to create budget");

    let allocation_sizes = vec![
        ("small", 1024),        // 1KB
        ("medium", 64 * 1024),  // 64KB
        ("large", 1024 * 1024), // 1MB
    ];

    for (name, size) in allocation_sizes {
        group.bench_with_input(BenchmarkId::new("allocate_and_free", name), &size, |b, &size| {
            b.iter(|| {
                if let Ok(Some(allocation)) = budget.try_allocate(size, "benchmark") {
                    black_box(&allocation);
                    // allocation automatically deallocated when dropped
                }
            })
        });
    }

    group.finish();
}

/// Benchmark memory pressure detection
fn bench_memory_pressure_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pressure_detection");

    let memory_limits = vec![
        ("unlimited", None),
        ("constrained_100mb", Some(100 * 1024 * 1024)),
        ("constrained_10mb", Some(10 * 1024 * 1024)),
    ];

    for (name, limit) in memory_limits {
        group.bench_with_input(BenchmarkId::new("detect_pressure", name), &limit, |b, &limit| {
            b.iter(|| black_box(detect_memory_pressure(limit)))
        });
    }

    group.finish();
}

/// Benchmark conservative strategy adaptation under memory pressure
fn bench_conservative_adaptation(c: &mut Criterion) {
    // Initialize config for the library
    tale_ndjson::config::set(ConfigOpts::default()).expect("Failed to initialize config");

    let mut group = c.benchmark_group("conservative_adaptation");

    let strategy = ConservativeStrategy::default();
    let chunk_sizes = vec![4096, 32768, 262144]; // 4KB, 32KB, 256KB

    for &chunk_size in &chunk_sizes {
        group.bench_with_input(
            BenchmarkId::new("adapt_under_pressure", format!("{}kb", chunk_size / 1024)),
            &chunk_size,
            |b, &size| {
                b.iter(|| {
                    let mut test_strategy = strategy.clone();
                    let metrics = tale_ndjson::metrics::ChunkMetrics::new();
                    black_box(test_strategy.adapt_size(&metrics, size))
                })
            },
        );
    }

    group.finish();
}

/// Benchmark memory budget statistics collection
fn bench_memory_statistics(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_statistics");

    // Create budget with some allocations
    let budget = MemoryBudget::new(100 * 1024 * 1024).expect("Failed to create budget");

    // Make a few allocations to have some stats
    let _alloc1 = budget
        .try_allocate(1024 * 1024, "test1")
        .expect("Failed to allocate")
        .expect("Allocation should succeed");
    let _alloc2 = budget
        .try_allocate(2 * 1024 * 1024, "test2")
        .expect("Failed to allocate")
        .expect("Allocation should succeed");

    group.bench_function("usage_stats", |b| {
        b.iter(|| black_box(budget.usage_stats().expect("Failed to get stats")))
    });

    group.bench_function("current_pressure", |b| {
        b.iter(|| black_box(budget.current_pressure().expect("Failed to get pressure")))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_budget_creation,
    bench_memory_allocation,
    bench_memory_pressure_detection,
    bench_conservative_adaptation,
    bench_memory_statistics
);

criterion_main!(benches);
