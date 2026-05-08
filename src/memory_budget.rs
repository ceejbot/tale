//! Memory budget management for chunked file processing.
//!
//! Provides a simple memory budget that tracks total usage against a limit
//! and reports pressure levels. Used by ChunkedFileReader to adjust chunk
//! sizes when memory is constrained.
//!
//! ## Usage
//! ```no_run
//! use tale_ndjson::{MemoryBudget, MemoryPressure};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let budget = MemoryBudget::new(100 * 1024 * 1024)?; // 100MB limit
//!
//! if let Some(allocation) = budget.try_allocate(4096)? {
//!     // Process with allocated memory
//!     allocation.deallocate(); // Automatic on drop
//! }
//!
//! match budget.current_pressure()? {
//!     MemoryPressure::Low => { /* normal operation */ },
//!     MemoryPressure::Moderate => { /* mild optimization */ },
//!     MemoryPressure::High => { /* reduce chunk sizes */ },
//!     MemoryPressure::Critical => { /* emergency measures */ },
//! }
//! # Ok(())
//! # }
//! ```

use std::sync::{Arc, RwLock};

use crate::errors::TaleError;

/// Memory pressure levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Memory usage < 60% of limit
    Low,
    /// Memory usage 60-85% of limit
    Moderate,
    /// Memory usage 85-95% of limit
    High,
    /// Memory usage > 95% of limit
    Critical,
}

/// A tracked memory allocation that frees on drop.
#[derive(Debug)]
pub struct MemoryAllocation {
    size: usize,
    budget: Arc<RwLock<BudgetState>>,
}

impl MemoryAllocation {
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn deallocate(self) {
        drop(self);
    }
}

impl Drop for MemoryAllocation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.budget.write() {
            state.current_usage = state.current_usage.saturating_sub(self.size);
        }
    }
}

/// Internal budget state
#[derive(Debug)]
struct BudgetState {
    total_limit: usize,
    current_usage: usize,
    peak_usage: usize,
}

impl BudgetState {
    fn pressure(&self) -> MemoryPressure {
        use crate::defaults::SystemDefaults;
        let ratio = self.current_usage as f64 / self.total_limit as f64;

        match ratio {
            r if r < SystemDefaults::MEMORY_PRESSURE_LOW_THRESHOLD => MemoryPressure::Low,
            r if r < SystemDefaults::MEMORY_PRESSURE_MODERATE_THRESHOLD => MemoryPressure::Moderate,
            r if r < SystemDefaults::MEMORY_PRESSURE_HIGH_THRESHOLD => MemoryPressure::High,
            _ => MemoryPressure::Critical,
        }
    }
}

/// Shared memory budget manager
#[derive(Debug, Clone)]
pub struct MemoryBudget {
    inner: Arc<RwLock<BudgetState>>,
}

impl MemoryBudget {
    /// Create a new memory budget with the specified limit (in bytes)
    pub fn new(total_limit: usize) -> Result<Self, TaleError> {
        Ok(Self {
            inner: Arc::new(RwLock::new(BudgetState {
                total_limit,
                current_usage: 0,
                peak_usage: 0,
            })),
        })
    }

    /// Create a memory budget based on a percentage of system memory
    pub fn from_system_memory(percentage: f64) -> Result<Self, TaleError> {
        let system_memory = if let Some(stats) = memory_stats::memory_stats() {
            stats.physical_mem
        } else {
            1024 * 1024 * 1024 // 1GB fallback
        };
        let limit = (system_memory as f64 * percentage / 100.0) as usize;
        Self::new(limit)
    }

    /// Try to allocate memory. Returns None if over budget.
    pub fn try_allocate(&self, size: usize) -> Result<Option<MemoryAllocation>, TaleError> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock".to_string()))?;

        let new_usage = state.current_usage + size;
        if new_usage > state.total_limit {
            return Ok(None);
        }

        state.current_usage = new_usage;
        state.peak_usage = state.peak_usage.max(new_usage);

        Ok(Some(MemoryAllocation {
            size,
            budget: self.inner.clone(),
        }))
    }

    /// Get current memory pressure level
    pub fn current_pressure(&self) -> Result<MemoryPressure, TaleError> {
        let state = self
            .inner
            .read()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock".to_string()))?;
        Ok(state.pressure())
    }

    /// Get current memory usage statistics
    pub fn usage_stats(&self) -> Result<MemoryBudgetStats, TaleError> {
        let state = self
            .inner
            .read()
            .map_err(|_| TaleError::MemoryError("Failed to acquire budget lock".to_string()))?;

        Ok(MemoryBudgetStats {
            total_limit: state.total_limit,
            current_usage: state.current_usage,
            peak_usage: state.peak_usage,
            pressure: state.pressure(),
        })
    }
}

/// Memory budget usage statistics
#[derive(Debug, Clone)]
pub struct MemoryBudgetStats {
    pub total_limit: usize,
    pub current_usage: usize,
    pub peak_usage: usize,
    pub pressure: MemoryPressure,
}

impl MemoryBudgetStats {
    pub fn usage_percentage(&self) -> f64 {
        if self.total_limit > 0 {
            (self.current_usage as f64 / self.total_limit as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn available_memory(&self) -> usize {
        self.total_limit.saturating_sub(self.current_usage)
    }

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_budget_allocation_works() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?;

        let alloc1 = budget.try_allocate(500)?;
        assert!(alloc1.is_some());

        let alloc2 = budget.try_allocate(400)?;
        assert!(alloc2.is_some());

        // Exceeds limit
        let alloc3 = budget.try_allocate(200)?;
        assert!(alloc3.is_none());

        // After dropping first, should succeed
        drop(alloc1);
        let alloc4 = budget.try_allocate(300)?;
        assert!(alloc4.is_some());

        Ok(())
    }

    #[test]
    fn memory_pressure_calculation_works() -> Result<(), TaleError> {
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
    fn allocation_automatic_cleanup_works() -> Result<(), TaleError> {
        let budget = MemoryBudget::new(1000)?;

        {
            let _alloc1 = budget.try_allocate(500)?;
            let _alloc2 = budget.try_allocate(400)?;
            let stats = budget.usage_stats()?;
            assert_eq!(stats.current_usage, 900);
        }

        let stats = budget.usage_stats()?;
        assert_eq!(stats.current_usage, 0);

        Ok(())
    }
}
