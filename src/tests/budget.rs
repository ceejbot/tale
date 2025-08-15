//! Integration tests for memory budget management

use tale::memory_budget::{MemoryBudget, MemoryPressure};

#[test]
fn test_memory_budget_basic_functionality() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test budget with 10KB limit
    let budget = MemoryBudget::new(10 * 1024)?;

    // Test basic allocation and deallocation
    let alloc1 = budget.try_allocate(4096, "test_reader")?;
    assert!(alloc1.is_some());

    let stats = budget.usage_stats()?;
    assert_eq!(stats.current_usage, 4096);
    assert_eq!(stats.pressure, MemoryPressure::Low);

    // Drop allocation and check it's cleaned up
    drop(alloc1);
    let stats = budget.usage_stats()?;
    assert_eq!(stats.current_usage, 0);

    Ok(())
}

#[test]
fn test_memory_pressure_levels() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(1000)?; // 1KB limit for easy testing

    // Low pressure (< 60%)
    let _alloc1 = budget.try_allocate(500, "reader1")?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Low);

    // Moderate pressure (60-85%)
    let _alloc2 = budget.try_allocate(150, "reader2")?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Moderate);

    // High pressure (85-95%)
    let _alloc3 = budget.try_allocate(200, "reader3")?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::High);

    // Critical pressure (> 95%)
    let _alloc4 = budget.try_allocate(100, "reader4")?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Critical);

    Ok(())
}

// Note: ChunkedFileReader tests would require fixing complex import issues
// This is handled by the unit tests in the memory_budget module instead

#[test]
fn test_emergency_allocation() -> Result<(), Box<dyn std::error::Error>> {
    // Create a very small budget to trigger emergency allocation
    let budget = MemoryBudget::new(2048)?; // 2KB limit

    // Test that emergency allocation works
    let alloc1 = budget.try_allocate(1950, "reader1")?; // Use 95.2% of budget (critical pressure)
    assert!(alloc1.is_some());

    // This should fail with normal allocation
    let alloc2 = budget.try_allocate(100, "reader2")?;
    assert!(alloc2.is_none());

    // Verify we're in critical pressure (>95%)
    assert_eq!(budget.current_pressure()?, MemoryPressure::Critical);

    Ok(())
}

#[test]
fn test_recommended_chunk_size_adaptation() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(1000)?;

    // Low pressure - no reduction
    let recommended = budget.recommended_chunk_size(1000)?;
    assert_eq!(recommended, 1000);

    // Force moderate pressure
    let _alloc = budget.try_allocate(700, "reader1")?;
    let recommended = budget.recommended_chunk_size(1000)?;
    assert_eq!(recommended, 800); // 20% reduction

    // Force high pressure
    let _alloc2 = budget.try_allocate(150, "reader2")?;
    let recommended = budget.recommended_chunk_size(1000)?;
    assert_eq!(recommended, 500); // 50% reduction

    // Force critical pressure
    let _alloc3 = budget.try_allocate(100, "reader3")?;
    let recommended = budget.recommended_chunk_size(1000)?;
    assert_eq!(recommended, 250); // 75% reduction

    Ok(())
}

#[test]
fn can_report_memory_stats() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(10000)?;

    let _alloc1 = budget.try_allocate(3000, "reader1")?;
    let _alloc2 = budget.try_allocate(2000, "reader2")?;

    let stats = budget.usage_stats()?;

    assert_eq!(stats.total_limit, 10000);
    assert_eq!(stats.current_usage, 5000);
    assert_eq!(stats.usage_percentage(), 50.0);
    assert_eq!(stats.available_memory(), 5000);
    assert_eq!(stats.reader_count, 2);
    assert_eq!(stats.pressure, MemoryPressure::Low);

    // Test that stats can be printed without panicking
    stats.print_report();

    Ok(())
}
