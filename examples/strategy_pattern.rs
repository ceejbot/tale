//! Strategy pattern example for chunk sizing
//!
//! This example demonstrates how different strategies adapt
//! chunk sizes for optimal file processing performance.
//!
//! Shows:
//! - Strategy selection based on file size
//! - Realistic performance metrics simulation
//! - Adaptation behavior under different conditions

use std::time::Duration;

use tale_ndjson::config::ConfigOpts;
use tale_ndjson::metrics::ChunkMetrics;
use tale_ndjson::readers::strategies::{AdaptiveStrategy, IsStrategy, Strategy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize configuration required by the library
    tale_ndjson::config::set(ConfigOpts::default()).expect("Failed to initialize config");

    println!("Strategy Pattern Example");

    // Create different file scenarios
    let scenarios = vec![
        ("Small file", Some(50 * 1024)),         // 50KB
        ("Medium file", Some(10 * 1024 * 1024)), // 10MB
        ("Large file", Some(500 * 1024 * 1024)), // 500MB
        ("Unknown size", None),
    ];

    for (description, file_size) in scenarios {
        println!("\n--- {} scenario ---", description);
        demonstrate_strategy_selection(file_size)?;
    }

    println!("\n--- Strategy Adaptation Demo ---");
    demonstrate_adaptation()?;

    Ok(())
}

fn demonstrate_strategy_selection(file_size: Option<u64>) -> Result<(), Box<dyn std::error::Error>> {
    let config = ConfigOpts::default();
    let strategy = Strategy::from_config(&config, file_size);

    match file_size {
        Some(size) => println!("File size: {} bytes", size),
        None => println!("File size: Unknown"),
    }

    println!("Selected strategy: {:?}", strategy);
    println!("Initial chunk size: {} bytes", strategy.initial_chunk_size());

    // Show strategy characteristics and reasoning
    match strategy {
        Strategy::Static(ref _s) => {
            println!("Static strategy - fixed chunk size optimized for file size");
            println!("  • Best for: Single-file processing with predictable I/O");
            println!("  • Behavior: Never adapts - consistent performance");
        }
        Strategy::Adaptive(ref _s) => {
            println!("Adaptive strategy - dynamic sizing based on performance metrics");
            println!("  • Best for: Variable workloads with changing conditions");
            println!("  • Behavior: Grows chunks when fast, shrinks when slow/memory pressure");
        }
        Strategy::Conservative(ref _s) => {
            println!("Conservative strategy - memory-constrained optimization");
            println!("  • Best for: Low-memory environments (containers, edge devices)");
            println!("  • Behavior: Prioritizes memory efficiency over throughput");
        }
    }

    Ok(())
}

fn demonstrate_adaptation() -> Result<(), Box<dyn std::error::Error>> {
    // Create an adaptive strategy for demonstration
    let mut strategy = AdaptiveStrategy::default();
    let initial_size = strategy.initial_chunk_size();
    println!(
        "Created AdaptiveStrategy with {}KB initial chunk size",
        initial_size / 1024
    );

    // Simulate performance scenarios
    println!("\n--- Scenario 1: Fast Processing Performance ---");
    let fast_metrics = create_fast_metrics();
    println!("Simulated 5 chunks: 32KB each processed in 3-5ms (efficient I/O)");
    let new_size = strategy.adapt_size(&fast_metrics, initial_size);
    println!(
        "Result: {:.1}MB/s avg speed → recommended chunk size: {}KB ({}x growth)",
        fast_metrics.processing_speed_mbps(),
        new_size / 1024,
        new_size as f64 / initial_size as f64
    );

    println!("\n--- Scenario 2: Slow Processing Performance ---");
    let slow_metrics = create_slow_metrics();
    println!("Simulated 5 chunks: 128KB each processed in 50-70ms (I/O bottleneck)");
    let new_size = strategy.adapt_size(&slow_metrics, initial_size);
    println!(
        "Result: {:.1}MB/s avg speed → recommended chunk size: {}KB ({}x shrink)",
        slow_metrics.processing_speed_mbps(),
        new_size / 1024,
        new_size as f64 / initial_size as f64
    );

    println!("\n--- Scenario 3: Degrading Performance Over Time ---");
    demonstrate_adaptation_cycle()?;

    Ok(())
}

/// Create metrics showing fast processing performance
/// Simulates efficient processing with good throughput
fn create_fast_metrics() -> ChunkMetrics {
    let mut metrics = ChunkMetrics::new();

    // Simulate several fast processing cycles
    // Small chunks, short processing times, good throughput
    metrics.record_chunk_processing(32 * 1024, Duration::from_millis(5), 150); // 32KB in 5ms
    metrics.record_chunk_processing(32 * 1024, Duration::from_millis(4), 160); // Getting faster
    metrics.record_chunk_processing(32 * 1024, Duration::from_millis(3), 170); // Even better
    metrics.record_chunk_processing(32 * 1024, Duration::from_millis(4), 155); // Consistent
    metrics.record_chunk_processing(32 * 1024, Duration::from_millis(3), 165); // Still good

    metrics
}

/// Create metrics showing slow processing performance  
/// Simulates I/O bottlenecks or memory pressure
fn create_slow_metrics() -> ChunkMetrics {
    let mut metrics = ChunkMetrics::new();

    // Simulate slow processing cycles
    // Large chunks taking a long time, indicating bottlenecks
    metrics.record_chunk_processing(128 * 1024, Duration::from_millis(50), 80); // 128KB in 50ms - slow
    metrics.record_chunk_processing(128 * 1024, Duration::from_millis(60), 75); // Getting worse  
    metrics.record_chunk_processing(128 * 1024, Duration::from_millis(70), 70); // Even slower
    metrics.record_chunk_processing(128 * 1024, Duration::from_millis(65), 72); // Still bad
    metrics.record_chunk_processing(128 * 1024, Duration::from_millis(68), 74); // Consistently slow

    metrics
}

/// Demonstrate how strategy adapts over multiple cycles
fn demonstrate_adaptation_cycle() -> Result<(), Box<dyn std::error::Error>> {
    let mut strategy = AdaptiveStrategy::default();
    let mut current_size = strategy.initial_chunk_size();

    println!("Starting with {}KB chunks", current_size / 1024);

    // Phase 1: Good performance - should grow chunks
    println!("\nPhase 1: Good performance (should grow chunks)");
    let good_metrics = create_fast_metrics();
    current_size = strategy.adapt_size(&good_metrics, current_size);
    println!("  → Adapted to {}KB chunks", current_size / 1024);

    // Phase 2: Even better performance - should grow more
    println!("\nPhase 2: Excellent performance (should grow more)");
    let excellent_metrics = create_excellent_metrics();
    current_size = strategy.adapt_size(&excellent_metrics, current_size);
    println!("  → Adapted to {}KB chunks", current_size / 1024);

    // Phase 3: Performance degrades - should shrink
    println!("\nPhase 3: Performance degrades (should shrink)");
    let degraded_metrics = create_slow_metrics();
    current_size = strategy.adapt_size(&degraded_metrics, current_size);
    println!("  → Adapted to {}KB chunks", current_size / 1024);

    println!("\nAdaptation cycle complete - strategy responded to changing conditions");

    Ok(())
}

/// Create metrics showing excellent processing performance
/// Even better than fast metrics to trigger growth
fn create_excellent_metrics() -> ChunkMetrics {
    let mut metrics = ChunkMetrics::new();

    // Simulate excellent processing - consistently fast and improving
    metrics.record_chunk_processing(64 * 1024, Duration::from_millis(4), 300); // 64KB in 4ms
    metrics.record_chunk_processing(64 * 1024, Duration::from_millis(3), 320); // Getting better
    metrics.record_chunk_processing(64 * 1024, Duration::from_millis(3), 310); // Consistently excellent  
    metrics.record_chunk_processing(64 * 1024, Duration::from_millis(2), 330); // Even faster
    metrics.record_chunk_processing(64 * 1024, Duration::from_millis(2), 325); // Sustained performance

    metrics
}
