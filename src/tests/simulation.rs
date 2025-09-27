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

use std::io;
use std::thread;
use std::time::Duration;

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

#[allow(dead_code)]
impl _StressedIoSimulator {
    pub fn new<T: Into<Vec<u8>>>(data: T) -> Self {
        Self {
            data: data.into(),
            position: 0,
            latency_ms: 0,
            bandwidth_limit: None,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.data.len() {
            return Ok(0);
        }

        if self.latency_ms > 0 {
            thread::sleep(Duration::from_millis(self.latency_ms));
        }

        let available = self.data.len() - self.position;
        let bandwidth_cap = self.bandwidth_limit.unwrap_or(available);
        let to_copy = available.min(bandwidth_cap).min(buf.len());

        let end = self.position + to_copy;
        buf[..to_copy].copy_from_slice(&self.data[self.position..end]);
        self.position = end;

        Ok(to_copy)
    }

    pub fn add_latency(&mut self, ms: u64) {
        self.latency_ms = ms;
    }

    pub fn throttle_bandwidth(&mut self, bytes_per_sec: usize) {
        if bytes_per_sec == 0 {
            self.bandwidth_limit = None;
        } else {
            self.bandwidth_limit = Some(bytes_per_sec);
        }
    }
}

impl _IoSimulator for _StressedIoSimulator {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        <_StressedIoSimulator>::read(self, buf)
    }

    fn add_latency(&mut self, ms: u64) {
        <_StressedIoSimulator>::add_latency(self, ms);
    }

    fn throttle_bandwidth(&mut self, bytes_per_sec: usize) {
        <_StressedIoSimulator>::throttle_bandwidth(self, bytes_per_sec);
    }
}

#[cfg(test)]
pub mod mock_mem_impl {
    use std::sync::{LazyLock, Mutex};

    use crate::metrics::MemoryPressure;

    const BYTES_PER_MB: usize = 1024 * 1024;
    const DEFAULT_SYSTEM_MEMORY_MB: usize = 1024;
    const DEFAULT_LIMIT_MB: usize = 200;

    static MOCK_MEMORY_MB: LazyLock<Mutex<usize>> = LazyLock::new(|| Mutex::new(0));

    fn limit_bytes(max_allowed_bytes: Option<usize>) -> usize {
        max_allowed_bytes.unwrap_or(DEFAULT_LIMIT_MB * BYTES_PER_MB)
    }

    pub fn detect_memory_pressure(max_allowed_bytes: Option<usize>) -> MemoryPressure {
        let usage_bytes = process_memory_bytes();
        let limit = limit_bytes(max_allowed_bytes);
        if limit == 0 {
            return MemoryPressure::Unknown;
        }

        let percent = usage_bytes as f64 * 100.0 / limit as f64;
        MemoryPressure::from(percent)
    }

    pub fn get_system_ram_bytes() -> usize {
        DEFAULT_SYSTEM_MEMORY_MB * BYTES_PER_MB
    }

    pub fn available_memory_bytes() -> usize {
        let system_remaining = get_system_ram_bytes().saturating_sub(process_memory_bytes());
        let budget_remaining = limit_bytes(None).saturating_sub(process_memory_bytes());
        system_remaining.min(budget_remaining)
    }

    pub fn process_memory_bytes() -> usize {
        current_memory_mb() * BYTES_PER_MB
    }

    pub fn set_mock_memory_mb(mb: usize) {
        *MOCK_MEMORY_MB.lock().expect("mock memory poisoned: set") = mb;
    }

    pub fn _get_memory_mb() -> usize {
        current_memory_mb()
    }

    fn current_memory_mb() -> usize {
        *MOCK_MEMORY_MB.lock().expect("mock memory poisoned: get")
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
    fn pressure_release_works() {
        let mut strategy = AdaptiveStrategy::default();
        // Simulate critical memory
        mock_mem_impl::set_mock_memory_mb(195); // If limit is 200
        let metrics = ChunkMetrics::new();
        let new_size = strategy.adapt_size(&metrics, 256_000);
        assert_eq!(new_size, strategy.config.min_chunk_size); // Should drop to minimum
    }
}
