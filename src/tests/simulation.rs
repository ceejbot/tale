//! Simulate file & OS data so we can control all parameters.

/*

Pre-release Testing notes

1. Performance Regression Suite
  - Compare against GNU tail on various file sizes
  - Measure memory usage under different scenarios
  - Track adaptation effectiveness
2. Stress Scenarios
  - High memory pressure
  - Slow I/O (network filesystems)
  - Rapid file growth
  - Many small files vs few large files
3. Platform Testing
  - Linux (your primary target)
  - macOS (development platform)
  - Windows (future - via WSL initially?)
*/

pub trait _IoSimulator {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn add_latency(&mut self, ms: u64);
    fn throttle_bandwidth(&mut self, bytes_per_sec: usize);
}

#[derive(Debug, Clone)]
pub struct _StressedIoSimulator {
    data: Vec<u8>,
    position: usize,
    latency_ms: u64,
    bandwidth_limit: Option<usize>,
}

#[cfg(test)]
pub mod mock_mem_impl {
    use std::sync::{LazyLock, Mutex};

    use crate::metrics::MemoryPressure;

    static MOCK_MEMORY: LazyLock<Mutex<Option<usize>>> = LazyLock::new(Default::default);

    pub fn _detect_memory_pressure(_max_allowed_bytes: Option<usize>) -> MemoryPressure {
        todo!()
    }

    pub fn _get_system_ram_bytes() -> usize {
        todo!()
    }

    pub fn _available_memory_bytes() -> usize {
        todo!()
    }

    pub fn _process_memory_bytes() -> usize {
        todo!()
    }

    pub fn set_mock_memory_mb(mb: usize) {
        *MOCK_MEMORY.lock().expect("mock memory poisoned: set") = Some(mb);
    }

    pub fn _get_memory_mb() -> usize {
        MOCK_MEMORY.lock().expect("mock memory poisoned: get").unwrap_or(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::*;
    use crate::readers::*;

    #[test]
    fn adapts_under_memory_pressure() {
        // Simulate gradual memory increase
        mock_mem_impl::set_mock_memory_mb(50);
        // ... run chunks, verify normal size

        mock_mem_impl::set_mock_memory_mb(150);
        // ... verify shrinking

        mock_mem_impl::set_mock_memory_mb(190);
        // ... verify minimum size
    }

    #[test]
    fn handles_slow_io_gracefully() {
        // use the api above like this:
        // let mut sim = StressedIoSimulator::new(test_data);
        // sim.add_latency(500); // 500ms per read
        // ... verify adaptation
    }

    #[test]
    #[ignore] // TODO: Fix mock memory implementation for pressure testing
    fn pressure_release_works() {
        let mut strategy = AdaptiveStrategy::default();
        // Simulate critical memory
        mock_mem_impl::set_mock_memory_mb(195); // If limit is 200
        let metrics = ChunkMetrics::new();
        let new_size = strategy.adapt_size(&metrics, 256_000);
        assert_eq!(new_size, strategy.config.min_chunk_size); // Should drop to minimum
    }
}
