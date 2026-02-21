//! Basic chunk processing metrics for diagnostics.

use std::time::Duration;

/// Basic metrics for chunk processing.
#[derive(Debug, Clone)]
pub struct ChunkMetrics {
    /// Number of chunks processed
    pub chunks_seen: usize,
    /// Total bytes processed
    pub total_bytes: usize,
    /// Total time spent processing
    pub total_duration: Duration,
}

impl Default for ChunkMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkMetrics {
    pub fn new() -> Self {
        Self {
            chunks_seen: 0,
            total_bytes: 0,
            total_duration: Duration::new(0, 0),
        }
    }

    pub fn record_chunk(&mut self, chunk_size: usize, duration: Duration) {
        self.chunks_seen += 1;
        self.total_bytes += chunk_size;
        self.total_duration += duration;
    }

    pub fn overall_throughput_mbps(&self) -> f64 {
        let total_mb = self.total_bytes as f64 / (1024.0 * 1024.0);
        let total_secs = self.total_duration.as_secs_f64();
        if total_secs > 0.0 { total_mb / total_secs } else { 0.0 }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic_metrics_work() {
        let mut metrics = ChunkMetrics::new();

        metrics.record_chunk(8192, Duration::from_millis(10));
        metrics.record_chunk(8192, Duration::from_millis(12));
        metrics.record_chunk(8192, Duration::from_millis(8));

        assert_eq!(metrics.chunks_seen, 3);
        assert_eq!(metrics.total_bytes, 8192 * 3);
        assert!(metrics.overall_throughput_mbps() > 0.0);
    }
}
