//! Integration tests for memory budget management

use crate::memory_budget::{MemoryBudget, MemoryPressure};

#[test]
fn memory_budget_basic_features_work() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(10 * 1024)?;

    let alloc1 = budget.try_allocate(4096)?;
    assert!(alloc1.is_some());

    let stats = budget.usage_stats()?;
    assert_eq!(stats.current_usage, 4096);
    assert_eq!(stats.pressure, MemoryPressure::Low);

    drop(alloc1);
    let stats = budget.usage_stats()?;
    assert_eq!(stats.current_usage, 0);

    Ok(())
}

#[test]
fn test_memory_pressure_levels() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(1000)?;

    let _alloc1 = budget.try_allocate(500)?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Low);

    let _alloc2 = budget.try_allocate(150)?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Moderate);

    let _alloc3 = budget.try_allocate(200)?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::High);

    let _alloc4 = budget.try_allocate(100)?;
    assert_eq!(budget.current_pressure()?, MemoryPressure::Critical);

    Ok(())
}

#[test]
fn can_do_emergency_allocation() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(2048)?;

    let alloc1 = budget.try_allocate(1950)?;
    assert!(alloc1.is_some());

    // Should fail — over budget
    let alloc2 = budget.try_allocate(100)?;
    assert!(alloc2.is_none());

    assert_eq!(budget.current_pressure()?, MemoryPressure::Critical);

    Ok(())
}

#[test]
fn can_report_memory_stats() -> Result<(), Box<dyn std::error::Error>> {
    let budget = MemoryBudget::new(10000)?;

    let _alloc1 = budget.try_allocate(3000)?;
    let _alloc2 = budget.try_allocate(2000)?;

    let stats = budget.usage_stats()?;

    assert_eq!(stats.total_limit, 10000);
    assert_eq!(stats.current_usage, 5000);
    assert_eq!(stats.usage_percentage(), 50.0);
    assert_eq!(stats.available_memory(), 5000);
    assert_eq!(stats.pressure, MemoryPressure::Low);

    stats.print_report();

    Ok(())
}
