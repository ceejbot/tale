//! Memory budget management example
//!
//! This example shows how to use the MemoryBudget system to track
//! and manage memory usage during file processing.

use tale_ndjson::{MemoryBudget, MemoryPressure, TaleError, config};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize configuration required by the library
    tale_ndjson::config::set(config::ConfigOpts::default()).expect("Failed to initialize config");

    println!("Memory Budget Management Example");

    // Create a memory budget with 10MB limit
    let mut budget = MemoryBudget::new(10 * 1024 * 1024)?;

    println!("Created memory budget with 10MB limit");

    // Simulate allocating memory for different readers
    simulate_reader_allocations(&mut budget, "log_reader_1", 1024 * 1024)?; // 1MB
    simulate_reader_allocations(&mut budget, "log_reader_2", 2 * 1024 * 1024)?; // 2MB  
    simulate_reader_allocations(&mut budget, "log_reader_3", 4 * 1024 * 1024)?; // 4MB

    // Try to allocate more memory and see pressure response
    println!("\n--- Attempting large allocation (5MB) ---");
    let chunk_size = 5 * 1024 * 1024;
    match budget.try_allocate(chunk_size, "large_reader")? {
        Some(allocation) => {
            println!("Successfully allocated {} bytes", chunk_size);

            // Check current pressure
            match budget.current_pressure()? {
                MemoryPressure::Low => println!("Memory pressure: Low - normal operation"),
                MemoryPressure::Moderate => println!("Memory pressure: Moderate - mild optimization recommended"),
                MemoryPressure::High => println!("Memory pressure: High - reduce chunk sizes"),
                MemoryPressure::Critical => println!("Memory pressure: Critical - emergency measures needed"),
            }

            // Clean up
            allocation.deallocate();
            println!("Deallocated memory");
        }
        None => {
            println!("Allocation failed - insufficient memory available");
            println!("Current pressure: {:?}", budget.current_pressure()?);
        }
    }

    Ok(())
}

fn simulate_reader_allocations(budget: &mut MemoryBudget, reader_name: &str, size: usize) -> Result<(), TaleError> {
    println!("\n--- Allocating memory for {} ---", reader_name);

    match budget.try_allocate(size, reader_name)? {
        Some(allocation) => {
            println!("Allocated {} bytes for {}", size, reader_name);

            // Check pressure after allocation
            let pressure = budget.current_pressure()?;
            println!("Current memory pressure: {:?}", pressure);

            // Keep allocation for demonstration
            // In real usage, allocation would be dropped automatically
            std::mem::forget(allocation); // Don't deallocate for demo
        }
        None => {
            println!("Failed to allocate {} bytes for {}", size, reader_name);
        }
    }

    Ok(())
}
