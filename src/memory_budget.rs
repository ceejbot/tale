//! Memory budget management for adaptive chunking
//!
//! This module provides comprehensive memory budget tracking and enforcement
//! for the chunked file processing system. It monitors system memory usage,
//! enforces per-process limits, and provides pressure-based adaptation signals.
//!
//! ## Key Features
//! - **Global memory budget**: System-wide memory limit enforcement
//! - **Per-reader tracking**: Individual memory usage monitoring
//! - **Pressure detection**: Multi-level memory pressure signals
//! - **Adaptive response**: Automatic chunk size and buffer adjustments
//! - **Graceful degradation**: Fallback strategies under memory pressure
//!
//! ## Usage
//! ```no_run
//! use tale_ndjson::{MemoryBudget, MemoryPressure};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut budget = MemoryBudget::new(100 * 1024 * 1024)?; // 100MB limit
//!
//! // Allocate memory for a chunk
//! let chunk_size = 4096;
//! if let Some(allocation) = budget.try_allocate(chunk_size, "reader_1")? {
//!     // Process with allocated memory
//!     allocation.deallocate(); // Automatic on drop
//! }
//!
//! // Check memory pressure
//! match budget.current_pressure()? {
//!     MemoryPressure::Low => { /* normal operation */ },
//!     MemoryPressure::Moderate => { /* mild optimization */ },
//!     MemoryPressure::High => { /* reduce chunk sizes */ },
//!     MemoryPressure::Critical => { /* emergency measures */ },
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::errors::TaleError;

/// Memory pressure levels for adaptive response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Memory usage < 60% of limit - normal operation
    Low,
    /// Memory usage 60-85% of limit - start reducing allocations
    Moderate,
    /// Memory usage 85-95% of limit - aggressive reduction
    High,
    /// Memory usage > 95% of limit - emergency measures
    Critical,
}

impl MemoryPressure {
    /// Get the adaptation factor for chunk sizes based on pressure level
    pub fn chunk_size_factor(&self) -> f64 {
        match self {
            MemoryPressure::Low => 1.0,       // No reduction
            MemoryPressure::Moderate => 0.8,  // 20% reduction
            MemoryPressure::High => 0.5,      // 50% reduction
            MemoryPressure::Critical => 0.25, // 75% reduction
        }
    }

    /// Whether to enable low-memory optimizations
    pub fn requires_optimization(&self) -> bool {
        matches!(self, MemoryPressure::High | MemoryPressure::Critical)
    }
}

/// Individual memory allocation tracking
#[derive(Debug)]
pub struct MemoryAllocation {
    size: usize,
    reader_id: String,
    allocated_at: Instant,
    budget: Arc<RwLock<MemoryBudgetInner>>,
}

impl MemoryAllocation {
    fn new(size: usize, reader_id: String, budget: Arc<RwLock<MemoryBudgetInner>>) -> Self {
        Self {
            size,
            reader_id,
            allocated_at: Instant::now(),
            budget,
        }
    }

    /// Get the size of this allocation
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get how long this allocation has been active
    pub fn age(&self) -> Duration {
        self.allocated_at.elapsed()
    }

    /// Manually deallocate this memory (automatic on drop)
    pub fn deallocate(self) {
        drop(self); // Explicit drop to trigger deallocation
    }
}

impl Drop for MemoryAllocation {
    fn drop(&mut self) {
        if let Ok(mut budget) = self.budget.write() {
            budget.deallocate(self.size, &self.reader_id);
        }
    }
}

/// Per-reader memory usage statistics
#[derive(Debug, Clone, Default)]
pub struct ReaderMemoryStats {
    /// Total bytes currently allocated
    pub current_allocation: usize,
    /// Peak allocation seen
    pub peak_allocation: usize,
    /// Number of active allocations
    pub allocation_count: usize,
    /// Number of times allocation failed
    pub allocation_failures: usize,
    /// Total allocations over lifetime
    pub total_allocations: usize,
}

/// Internal budget state
#[derive(Debug)]
struct MemoryBudgetInner {
    /// Maximum total memory allowed (in bytes)
    total_limit: usize,
    /// Currently allocated memory (in bytes)
    current_usage: usize,
    /// Peak memory usage seen
    peak_usage: usize,
    /// Per-reader memory tracking
    reader_stats: HashMap<String, ReaderMemoryStats>,
    /// System memory monitoring
    last_system_check: Instant,
    system_memory_available: usize,
}

impl MemoryBudgetInner {
    fn new(total_limit: usize) -> Result<Self, TaleError> {
        let system_memory = get_system_memory_available()?;

        Ok(Self {
            total_limit,
            current_usage: 0,
            peak_usage: 0,
            reader_stats: HashMap::new(),
            last_system_check: Instant::now(),
            system_memory_available: system_memory,
        })
    }

    fn try_allocate(&mut self, size: usize, reader_id: &str) -> Result<bool, TaleError> {
        // Update system memory if it's been a while
        if self.last_system_check.elapsed() > Duration::from_secs(1) {
            self.system_memory_available = get_system_memory_available()?;
            self.last_system_check = Instant::now();
        }

        // Check if allocation would exceed budget
        let new_usage = self.current_usage + size;
        if new_usage > self.total_limit {
            // Update failure stats
            let stats = self.reader_stats.entry(reader_id.to_string()).or_default();
            stats.allocation_failures += 1;
            return Ok(false);
        }

        // Check system memory availability (safety margin)
        let system_safety_margin = self.system_memory_available / 4; // Keep 25% free
        if size > system_safety_margin {
            let stats = self.reader_stats.entry(reader_id.to_string()).or_default();
            stats.allocation_failures += 1;
            return Ok(false);
        }

        // Allocation successful
        self.current_usage = new_usage;
        self.peak_usage = self.peak_usage.max(new_usage);

        // Update reader stats
        let stats = self.reader_stats.entry(reader_id.to_string()).or_default();
        stats.current_allocation += size;
        stats.peak_allocation = stats.peak_allocation.max(stats.current_allocation);
        stats.allocation_count += 1;
        stats.total_allocations += 1;

        Ok(true)
    }

    fn deallocate(&mut self, size: usize, reader_id: &str) {
        self.current_usage = self.current_usage.saturating_sub(size);

        if let Some(stats) = self.reader_stats.get_mut(reader_id) {
            stats.current_allocation = stats.current_allocation.saturating_sub(size);
            stats.allocation_count = stats.allocation_count.saturating_sub(1);
        }
    }

    fn current_pressure(&self) -> MemoryPressure {
        use crate::defaults::SystemDefaults;
        let usage_ratio = self.current_usage as f64 / self.total_limit as f64;

        match usage_ratio {
            r if r < SystemDefaults::MEMORY_PRESSURE_LOW_THRESHOLD => MemoryPressure::Low,
            r if r < SystemDefaults::MEMORY_PRESSURE_MODERATE_THRESHOLD => MemoryPressure::Moderate,
            r if r < SystemDefaults::MEMORY_PRESSURE_HIGH_THRESHOLD => MemoryPressure::High,
            _ => MemoryPressure::Critical,
        }
    }
}

/// Global memory budget manager for chunked file processing
#[derive(Debug, Clone)]
pub struct MemoryBudget {
    inner: Arc<RwLock<MemoryBudgetInner>>,
}

impl MemoryBudget {
    /// Create a new memory budget with the specified limit
    pub fn new(total_limit: usize) -> Result<Self, TaleError> {
        let inner = Arc::new(RwLock::new(MemoryBudgetInner::new(total_limit)?));
        Ok(Self { inner })
    }

    /// Create a memory budget based on system memory
    pub fn from_system_memory(percentage: f64) -> Result<Self, TaleError> {
        let system_memory = get_system_memory_available()?;
        let limit = (system_memory as f64 * percentage / 100.0) as usize;
        Self::new(limit)
    }

    /// Try to allocate memory for a specific reader
    pub fn try_allocate(&self, size: usize, reader_id: &str) -> Result<Option<MemoryAllocation>, TaleError> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock for allocation".to_string()))?;

        if inner.try_allocate(size, reader_id)? {
            let allocation = MemoryAllocation::new(size, reader_id.to_string(), self.inner.clone());
            Ok(Some(allocation))
        } else {
            Ok(None)
        }
    }

    /// Get current memory pressure level
    pub fn current_pressure(&self) -> Result<MemoryPressure, TaleError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock for pressure check".to_string()))?;
        Ok(inner.current_pressure())
    }

    /// Get current memory usage statistics
    pub fn usage_stats(&self) -> Result<MemoryBudgetStats, TaleError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock for stats".to_string()))?;

        Ok(MemoryBudgetStats {
            total_limit: inner.total_limit,
            current_usage: inner.current_usage,
            peak_usage: inner.peak_usage,
            pressure: inner.current_pressure(),
            reader_count: inner.reader_stats.len(),
            system_memory_available: inner.system_memory_available,
        })
    }

    /// Get memory statistics for a specific reader
    pub fn reader_stats(&self, reader_id: &str) -> Result<Option<ReaderMemoryStats>, TaleError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock for reader stats".to_string()))?;
        Ok(inner.reader_stats.get(reader_id).cloned())
    }

    /// Get recommended chunk size based on current memory pressure
    pub fn recommended_chunk_size(&self, base_size: usize) -> Result<usize, TaleError> {
        let pressure = self.current_pressure()?;
        let factor = pressure.chunk_size_factor();
        Ok((base_size as f64 * factor) as usize)
    }

    /// Check if emergency measures should be taken
    pub fn requires_emergency_measures(&self) -> Result<bool, TaleError> {
        let pressure = self.current_pressure()?;
        Ok(matches!(pressure, MemoryPressure::Critical))
    }
}

/// Memory budget usage statistics
#[derive(Debug, Clone)]
pub struct MemoryBudgetStats {
    /// Total memory limit
    pub total_limit: usize,
    /// Currently used memory
    pub current_usage: usize,
    /// Peak memory usage
    pub peak_usage: usize,
    /// Current memory pressure level
    pub pressure: MemoryPressure,
    /// Number of active readers
    pub reader_count: usize,
    /// Available system memory
    pub system_memory_available: usize,
}

impl MemoryBudgetStats {
    /// Get memory usage as a percentage
    pub fn usage_percentage(&self) -> f64 {
        if self.total_limit > 0 {
            (self.current_usage as f64 / self.total_limit as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get available memory
    pub fn available_memory(&self) -> usize {
        self.total_limit.saturating_sub(self.current_usage)
    }

    /// Print a formatted report
    pub fn print_report(&self) {
        println!("Memory Budget Report:");
        println!("====================");
        println!("Total Limit:    {} MB", self.total_limit / (1024 * 1024));
        println!(
            "Current Usage:  {} MB ({:.1}%)",
            self.current_usage / (1024 * 1024),
            self.usage_percentage()
        );
        println!("Peak Usage:     {} MB", self.peak_usage / (1024 * 1024));
        println!("Available:      {} MB", self.available_memory() / (1024 * 1024));
        println!("Pressure Level: {:?}", self.pressure);
        println!("Active Readers: {}", self.reader_count);
        println!("System Memory:  {} MB", self.system_memory_available / (1024 * 1024));
    }
}

/// Get available system memory
fn get_system_memory_available() -> Result<usize, TaleError> {
    // Try to get actual system memory stats
    if let Some(stats) = memory_stats::memory_stats() {
        // Use physical memory as a proxy for available memory
        // This is conservative but safe
        Ok(stats.physical_mem)
    } else {
        // Fallback: assume 1GB available (very conservative)
        Ok(1024 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn _memory_pressure_levels_work() {
        assert_eq!(MemoryPressure::Low.chunk_size_factor(), 1.0);
        assert_eq!(MemoryPressure::Moderate.chunk_size_factor(), 0.8);
        assert_eq!(MemoryPressure::High.chunk_size_factor(), 0.5);
        assert_eq!(MemoryPressure::Critical.chunk_size_factor(), 0.25);
    }

    #[test]
    fn memory_budget_allocation_works() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?; // 1KB limit

        // First allocation should succeed
        let alloc1 = budget.try_allocate(500, "reader1")?;
        assert!(alloc1.is_some());

        // Second allocation within limit should succeed
        let alloc2 = budget.try_allocate(400, "reader2")?;
        assert!(alloc2.is_some());

        // Third allocation exceeding limit should fail
        let alloc3 = budget.try_allocate(200, "reader3")?;
        assert!(alloc3.is_none());

        // After dropping first allocation, new allocation should succeed
        drop(alloc1);
        let alloc4 = budget.try_allocate(300, "reader4")?;
        assert!(alloc4.is_some());

        Ok(())
    }

    #[test]
    fn memory_pressure_calculation_works() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?;

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

    #[test]
    fn can_recommend_chunk_size() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?;

        // Low pressure - no reduction
        assert_eq!(budget.recommended_chunk_size(1000)?, 1000);

        // Force moderate pressure
        let _alloc = budget.try_allocate(700, "reader1")?;
        assert_eq!(budget.recommended_chunk_size(1000)?, 800); // 20% reduction

        Ok(())
    }

    #[test]
    fn allocation_automatic_cleanup_works() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?;

        {
            let _alloc1 = budget.try_allocate(500, "reader1")?;
            let _alloc2 = budget.try_allocate(400, "reader2")?;
            let stats = budget.usage_stats()?;
            assert_eq!(stats.current_usage, 900);
        } // Allocations dropped here

        // After drop, usage should be 0
        let stats = budget.usage_stats()?;
        assert_eq!(stats.current_usage, 0);

        Ok(())
    }
}
