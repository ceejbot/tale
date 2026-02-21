//! Collecting system metrics and maintaining a sliding window.
//! This will be fun.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct MetricsCollector {
    metrics: ChunkMetrics,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: ChunkMetrics::new(),
        }
    }

    pub fn chunks_seen(&self) -> usize {
        self.metrics.chunks_seen
    }

    pub fn chunk_sizes_avg(&self) -> f64 {
        self.metrics.chunk_sizes.average()
    }

    pub fn processing_speed_mbps(&self) -> f64 {
        self.metrics.processing_speed_mbps()
    }

    pub fn record_chunk(&mut self, chunk_bytes: usize, elapsed_ms: Duration, line_count: usize) {
        self.metrics
            .record_chunk_processing(chunk_bytes, elapsed_ms, line_count)
    }

    pub fn ready_to_adapt(&self, interval: usize) -> bool {
        self.metrics.chunks_seen > 0
            && self.metrics.chunks_seen.is_multiple_of(interval)
            && self.metrics.lines_moving.count >= 3
    }

    pub fn snapshot(&self) -> &ChunkMetrics {
        &self.metrics
    }
}

/// We want to track how we're doing so we know when to shift down into second
/// gear and ride the clutch or when we're winding up into high rpms and need to
/// upshift.
#[derive(Debug, Clone)]
pub struct ChunkMetrics {
    /// Number of chunks processed
    pub chunks_seen: usize,
    /// JSON parsing time: megabytes per millisecond
    pub parsed_per_ms: f64,
    /// how we're tracking megabytes parsed per ms
    pub parsed_moving: MovingAverage<20>,
    /// memory usage in bytes
    pub memory_bytes: usize,
    pub memory_moving: MovingAverage<20>,
    /// Tracking chunk sizes over time
    pub chunk_sizes: MovingAverage<20>,
    // The number of lines per chunk we're seeing
    pub lines_per_chunk: usize,
    /// Tracking the lines per chunk
    pub lines_moving: MovingAverage<20>,
    /// Total bytes processed
    total_bytes: usize,
    /// Total time spent processing
    total_duration: Duration,
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
            chunk_sizes: MovingAverage::new(),
            parsed_per_ms: 0.0,
            parsed_moving: MovingAverage::new(),
            memory_bytes: 0,
            memory_moving: MovingAverage::new(),
            lines_per_chunk: 0,
            lines_moving: MovingAverage::new(),
            total_bytes: 0,
            total_duration: Duration::new(0, 0),
        }
    }

    pub fn record_chunk_processing(&mut self, chunk_size: usize, duration: Duration, lines: usize) {
        self.chunks_seen += 1;
        self.lines_per_chunk = lines;
        self.lines_moving.push(lines as f64);

        // parsed per ms; MB / s
        let mb_processed = chunk_size as f64 / (1024.0 * 1024.0);
        let seconds = duration.as_secs_f64();
        let mbps = if seconds > 0.0 { mb_processed / seconds } else { 0.0 };
        self.parsed_per_ms = mbps;
        self.parsed_moving.push(mbps);

        if let Some(stats) = memory_stats::memory_stats() {
            self.memory_bytes = stats.physical_mem;
            self.memory_moving.push(stats.physical_mem as f64);
        }

        // This is not IOwait, but a proxy: If processing is slow relative to
        // chunk size, we're likely I/O bound. Calculate efficiency metric instead:
        // We want a 50MB/s baseline.
        // let expected_parse_time_ms = chunk_size as f64 / 50_000.0;
        // let actual_time_ms = duration.as_millis() as f64;
        // let efficiency = (expected_parse_time_ms / actual_time_ms.max(1.0)).min(1.0);

        self.chunk_sizes.push(chunk_size as f64);
        self.total_bytes += chunk_size;
        self.total_duration += duration;
    }

    pub fn overall_throughput_mbps(&self) -> f64 {
        let total_mb = self.total_bytes as f64 / (1024.0 * 1024.0);
        let total_secs = self.total_duration.as_secs_f64();
        if total_secs > 0.0 { total_mb / total_secs } else { 0.0 }
    }

    pub fn processing_speed_mbps(&self) -> f64 {
        self.parsed_moving.average()
    }

    pub fn speed_moving(&self) -> &MovingAverage<20> {
        &self.parsed_moving
    }

    pub fn memory_moving(&self) -> &MovingAverage<20> {
        &self.memory_moving
    }

    pub fn chunk_sizes(&self) -> &MovingAverage<20> {
        &self.chunk_sizes
    }

    /// We check if we should adapt every often. This means "check if we should
    /// adapt", not "we should definitely adapt".
    pub fn should_adapt(&self, interval: usize) -> bool {
        // Adapt every N chunks, but only after we have enough data
        self.chunks_seen > 0 && self.chunks_seen.is_multiple_of(interval) && self.lines_moving.count >= 3
    }
}

#[derive(Debug, Clone)]
pub struct MovingAverage<const N: usize> {
    values: [f64; N],
    index: usize,
    count: usize,
}

impl<const N: usize> Default for MovingAverage<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MovingAverage<N> {
    pub fn new() -> Self {
        Self {
            values: [0.0; N],
            index: 0,
            count: 0,
        }
    }

    /// Here's some more frequency, Kenneth.
    pub fn push(&mut self, value: f64) {
        self.values[self.index] = value;
        self.index = (self.index + 1) % N; // wrappity bappity
        if self.count < N {
            self.count += 1;
        }
    }

    /// What's the average of whatever this is, Kenneth?
    pub fn average(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f64 = if self.count < N {
            self.values[0..self.count].iter().sum()
        } else {
            self.values.iter().sum()
        };
        sum / self.count as f64
    }

    /// What's the trend, Kenneth?
    pub fn trend(&self, stable: f64) -> Trend {
        if self.count < 2 {
            // We don't have enough data to know.
            return Trend::Unknown;
        }

        let mid = self.count / 2;

        // Well, I'm doing this the old-fashioned C way. My apologies..
        let oldest_idx = if self.count < N {
            0 // Buffer not full, start at beginning
        } else {
            self.index // Buffer full, oldest is at current index
        };

        let mut ptr = oldest_idx;
        let mut sum = 0.0;
        for _i in 0..mid {
            sum += self.values[ptr];
            ptr = (ptr + 1) % N
        }
        let older = sum / mid as f64;

        ptr = (oldest_idx + mid) % N;
        let mut sum = 0.0;
        for _i in 0..mid {
            sum += self.values[ptr];
            ptr = (ptr + 1) % N;
        }
        let newer = sum / mid as f64;

        // Compare and determine the difference with a threshold.
        // If newer & older are within 5% of each other, it's stable
        // If the gap is wider than last time, we're degrading.

        let percent_diff = if older != 0.0 {
            ((newer - older) / older.abs()) * 100.0
        } else {
            0.0
        };

        if percent_diff.abs() < stable {
            Trend::Stable
        } else if percent_diff > 0.0 {
            Trend::Improving
        } else {
            Trend::Degrading
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
    #[default]
    Unknown,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn metrics_collectors_work() {
        let mut collector = MetricsCollector::new();

        // Record some chunks
        collector.record_chunk(8192, Duration::from_millis(10), 100);
        collector.record_chunk(8192, Duration::from_millis(12), 95);
        collector.record_chunk(8192, Duration::from_millis(8), 105);

        // Should not be ready yet (interval is 5)
        assert!(!collector.ready_to_adapt(5));

        collector.record_chunk(8192, Duration::from_millis(11), 98);
        collector.record_chunk(8192, Duration::from_millis(9), 102);

        // Now should be ready (5 chunks processed)
        assert!(collector.ready_to_adapt(5));

        // Check metrics are reasonable
        let snapshot = collector.snapshot();
        assert!(snapshot.processing_speed_mbps() > 0.0);
    }

    #[test]
    fn moving_average_can_average() {
        let mut avg = MovingAverage::<5>::new();
        avg.push(1.0);
        avg.push(2.0);
        avg.push(3.0);
        assert_eq!(avg.average(), 2.0);

        avg.push(4.0);
        avg.push(5.0);
        assert_eq!(avg.average(), 3.0);

        avg.push(6.0); // Should evict 1.0
        assert_eq!(avg.average(), 4.0); // (2+3+4+5+6)/5
    }

    #[test]
    fn can_keep_up_with_depeche_mode() {
        let mut avg = MovingAverage::<4>::new();

        // Increasing values
        avg.push(1.0);
        avg.push(2.0);
        avg.push(3.0);
        avg.push(4.0);
        assert!(matches!(avg.trend(0.5), Trend::Improving));

        // Stable values
        let mut avg = MovingAverage::<4>::new();
        avg.push(5.0);
        avg.push(5.1);
        avg.push(4.9);
        avg.push(5.0);
        assert!(matches!(avg.trend(4.0), Trend::Stable));
    }

    #[test]
    fn can_pass_thresholds() {
        // Stable
        let mut avg = MovingAverage::<4>::new();
        avg.push(50.0);
        avg.push(51.0);
        avg.push(49.0);
        avg.push(50.0);
        assert!(matches!(avg.trend(5.0), Trend::Stable));

        // This is degrading now
        let mut avg = MovingAverage::<4>::new();
        avg.push(50.0);
        avg.push(51.0);
        avg.push(49.0);
        avg.push(50.0);
        assert!(matches!(avg.trend(0.5), Trend::Degrading));
    }
}
