/// Collecting system metrics and maintaining a sliding window.
/// This will be fun.
use std::time::Duration;

/// We want to track how we're doing so we know when to shift down into second
/// gear and ride the clutch or when we're winding up into high rpms and need to
/// upshift.
#[derive(Debug, Clone)]
pub struct ChunkMetrics {
    /// JSON parsing time per chunk; in milliseconds
    parsed_per_ms: f64,
    /// how we're tracking parsed per ms
    parsed_moving: MovingAverage<10>,
    /// memory usage in bytes
    memory_bytes: usize,
    memory_moving: MovingAverage<10>,
    /// my favorite OS stat to look at
    io_wait_time: usize,
    iowait_moving: MovingAverage<10>,
    // The number of lines per chunk we're seeing
    lines_per_chunk: u64,
    lines_moving: MovingAverage<10>,
}

impl ChunkMetrics {
    pub fn new() -> Self {
        Self {
            parsed_per_ms: 0.0,
            parsed_moving: MovingAverage::new(),
            memory_bytes: 0,
            memory_moving: MovingAverage::new(),
            io_wait_time: 0,
            iowait_moving: MovingAverage::new(),
            lines_per_chunk: 0,
            lines_moving: MovingAverage::new(),
        }
    }

    pub fn record_chunk_processing(&mut self, chunk_size: usize, duration: Duration, lines: u64) {
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
        let expected_parse_time_ms = chunk_size as f64 / 50_000.0;
        let actual_time_ms = duration.as_millis() as f64;
        let efficiency = (expected_parse_time_ms / actual_time_ms.max(1.0)).min(1.0);

        // If efficiency < 0.5, we're spending too much time waiting
        self.io_wait_time = ((1.0 - efficiency) * 100.0) as usize; // Percentage
        self.iowait_moving.push(self.io_wait_time as f64);
    }

    pub fn processing_speed_mbps(&self) -> f64 {
        self.parsed_moving.average()
    }

    /// We check if we should adapt every often. This means "check if we should adapt",
    /// not "we should definitely adapt".
    pub fn should_adapt(&self, interval: usize, chunks_processed: usize) -> bool {
        // Adapt every N chunks, but only after we have enough data
        chunks_processed > 0 && chunks_processed % interval == 0 && self.lines_moving.count >= 3
    }
}

#[derive(Debug, Clone)]
pub struct MovingAverage<const N: usize> {
    values: [f64; N],
    index: usize,
    count: usize,
}

impl<const N: usize> MovingAverage<N> {
    pub fn new() -> Self {
        Self {
            values: [0.0; N],
            index: 0,
            count: 0,
        }
    }

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
    pub fn trend(&mut self) -> Trend {
        if self.count < 2 {
            // We don't have enough data to know.
            return Trend::Stable;
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
        for i in 0..mid {
            sum += self.values[ptr];
            ptr = (ptr + 1) % N
        }
        let older = sum / mid as f64;

        ptr = (oldest_idx + mid) % N;
        let mut sum = 0.0;
        for i in 0..mid {
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

        if percent_diff.abs() < 5.0 {
            Trend::Stable
        } else if percent_diff > 0.0 {
            Trend::Improving
        } else {
            Trend::Degrading
        }
    }
}

#[derive(Debug, Clone)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
}

#[cfg(test)]
mod test {
    use super::*;

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
    fn can_track_depeche_mode() {
        let mut avg = MovingAverage::<4>::new();

        // Increasing values
        avg.push(1.0);
        avg.push(2.0);
        avg.push(3.0);
        avg.push(4.0);
        assert!(matches!(avg.trend(), Trend::Improving));

        // Stable values
        let mut avg = MovingAverage::<4>::new();
        avg.push(5.0);
        avg.push(5.1);
        avg.push(4.9);
        avg.push(5.0);
        assert!(matches!(avg.trend(), Trend::Stable));
    }
}
