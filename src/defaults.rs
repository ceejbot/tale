//! System defaults and configuration constants for Tale
//!
//! This module provides all configuration constants, system defaults, and
//! preset configurations for the Tale log processing tool. It consolidates
//! what were previously separate constants.rs and production_defaults.rs
//! modules.

use std::time::Duration;

/// Basic I/O and processing constants
pub mod io {
    use super::Duration;

    /// How long we wait before flushing data to stdout when tailing.
    pub const TAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

    /// Flush stdout when we've written at least this many lines.
    pub const FLUSH_LINE_COUNT: u16 = 40;

    /// Default capacity for line strings.
    pub const LINE_CAPACITY: usize = 512;

    /// Buffer size for reading from stdin/files.
    pub const READ_BUFFER_SIZE: usize = 8192;

    /// Default capacity for output byte buffers.
    pub const OUTPUT_BUFFER_CAPACITY: usize = 1024;

    /// The initial chunk size to use for adaptive chunked readers.
    pub const INITIAL_CHUNK_SIZE: usize = 32 * 1024; // 32K bytes
}

/// File processing thresholds and decision constants
pub mod processing {
    /// Large offset threshold - offsets above this suggest large file
    /// operations
    pub const LARGE_OFFSET_THRESHOLD: u64 = 10_000;

    /// File size requiring chunked processing when combined with large offset
    pub const CHUNKED_WITH_OFFSET_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB

    /// File size that always requires chunked processing regardless of offset
    pub const ALWAYS_CHUNKED_FILE_SIZE: u64 = 1024 * 1024 * 1024; // 1GB

    /// Number of chunks between strategy adaptation checks
    pub const ADAPTATION_INTERVAL: usize = 20;

    /// The traditional unix block size in bytes.
    pub const BLOCK_SIZE: u64 = 512;
}

/// Memory management constants
pub mod memory {
    /// Memory limit for line buffering in negative line offset mode.
    pub const MEMORY_LIMIT_BYTES: usize = 10 * 1024 * 1024; // 10MB
}

/// System defaults for chunked file processing and configuration management
pub struct SystemDefaults;

impl SystemDefaults {
    /// Default memory budget as percentage of system memory
    ///
    /// Benchmark results show 10% provides good balance:
    /// - Sufficient for most workloads
    /// - Leaves headroom for system operations
    /// - Scales appropriately with system size
    pub const DEFAULT_MEMORY_PERCENTAGE: f64 = 10.0;

    /// Minimum memory budget (absolute floor)
    ///
    /// Below this, performance degrades significantly
    pub const MIN_MEMORY_BUDGET: usize = 5 * 1024 * 1024; // 5MB

    /// Maximum memory budget (safety ceiling)
    ///
    /// Even on large systems, cap at reasonable limit
    pub const MAX_MEMORY_BUDGET: usize = 500 * 1024 * 1024; // 500MB

    /// Default chunk size for initial processing
    ///
    /// Based on benchmarks:
    /// - 32KB provides good initial performance
    /// - Small enough for quick adaptation
    /// - Large enough for efficient I/O
    pub const DEFAULT_CHUNK_SIZE: usize = 32 * 1024; // 32KB (block-aligned)

    /// Minimum chunk size (emergency floor)
    ///
    /// Below this, overhead dominates performance
    pub const MIN_CHUNK_SIZE: usize = 4 * 1024; // 4KB (one block)

    /// Maximum chunk size (performance ceiling)
    ///
    /// Larger chunks show diminishing returns
    pub const MAX_CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB

    /// Optimal chunk size for different file sizes
    pub fn optimal_chunk_for_file(file_size: u64) -> usize {
        // Use 4KB block size (standard for most modern filesystems)
        const BLOCK_SIZE: usize = 4096;

        let base_size = match file_size {
            // Tiny files (< 100KB): Minimal chunking
            0..=102_400 => Self::MIN_CHUNK_SIZE,

            // Small files (100KB - 1MB): Small chunks
            102_401..=1_048_576 => 8 * 1024,

            // Medium files (1MB - 10MB): Standard chunks
            1_048_577..=10_485_760 => Self::DEFAULT_CHUNK_SIZE,

            // Large files (10MB - 100MB): Larger chunks
            10_485_761..=104_857_600 => 128 * 1024,

            // Very large files (100MB - 1GB): Big chunks
            104_857_601..=1_073_741_824 => 512 * 1024,

            // Huge files (> 1GB): Maximum chunks
            _ => 1024 * 1024,
        };

        // Ensure block alignment
        base_size.div_ceil(BLOCK_SIZE) * BLOCK_SIZE
    }

    /// Get default strategy based on context
    pub fn default_strategy() -> &'static str {
        // Conservative strategy proved best in benchmarks:
        // - Only 20% slower than adaptive in best case
        // - Much more predictable memory usage
        // - Better handling of memory pressure
        "conservative"
    }

    /// Should use chunked processing by default?
    pub fn should_chunk_by_default(file_size: u64) -> bool {
        // Use chunked processing for files > 1MB
        // Benchmarks show minimal overhead, better memory control
        file_size > 1_048_576
    }

    /// Default batch window for multi-file processing (ms)
    pub const DEFAULT_BATCH_WINDOW_MS: u64 = 250;

    /// Default line buffer capacity
    pub const DEFAULT_LINE_CAPACITY: usize = 512;

    /// Default output buffer capacity
    pub const DEFAULT_OUTPUT_BUFFER_CAPACITY: usize = 4096;

    /// Adaptation interval (chunks between adaptation checks)
    pub const ADAPTATION_INTERVAL: usize = 20;

    /// Memory pressure thresholds (validated through benchmarking)
    pub const MEMORY_PRESSURE_LOW_THRESHOLD: f64 = 0.60; // < 60%: Normal
    pub const MEMORY_PRESSURE_MODERATE_THRESHOLD: f64 = 0.85; // 60-85%: Moderate
    pub const MEMORY_PRESSURE_HIGH_THRESHOLD: f64 = 0.95; // 85-95%: High
    // > 95%: Critical

    /// Chunk size reduction factors per pressure level
    pub const PRESSURE_FACTOR_LOW: f64 = 1.0; // No reduction
    pub const PRESSURE_FACTOR_MODERATE: f64 = 0.8; // 20% reduction
    pub const PRESSURE_FACTOR_HIGH: f64 = 0.5; // 50% reduction
    pub const PRESSURE_FACTOR_CRITICAL: f64 = 0.25; // 75% reduction

    /// Emergency allocation factor
    pub const EMERGENCY_ALLOCATION_FACTOR: f64 = 0.25; // 25% of requested size
}

/// System configuration presets
pub enum ConfigPreset {
    /// Optimized for small files and low memory systems
    LowMemory,
    /// Balanced performance and memory usage
    Balanced,
    /// Optimized for maximum performance
    HighPerformance,
    /// Optimized for memory-constrained environments
    Conservative,
}

/// Configuration settings for each preset
pub struct PresetSettings {
    pub memory_percentage: f64,
    pub max_memory_mb: usize,
    pub default_chunk_kb: usize,
    pub max_chunk_kb: usize,
    pub strategy: &'static str,
    pub force_chunked: bool,
    pub adaptation_interval: usize,
}

impl ConfigPreset {
    /// Get settings for this preset
    pub fn settings(&self) -> PresetSettings {
        match self {
            ConfigPreset::LowMemory => PresetSettings {
                memory_percentage: 5.0,
                max_memory_mb: 50,
                default_chunk_kb: 16,
                max_chunk_kb: 256,
                strategy: "conservative",
                force_chunked: true,
                adaptation_interval: 10,
            },

            ConfigPreset::Balanced => PresetSettings {
                memory_percentage: 10.0,
                max_memory_mb: 200,
                default_chunk_kb: 32,
                max_chunk_kb: 2048,
                strategy: "conservative",
                force_chunked: false,
                adaptation_interval: 20,
            },

            ConfigPreset::HighPerformance => PresetSettings {
                memory_percentage: 20.0,
                max_memory_mb: 500,
                default_chunk_kb: 128,
                max_chunk_kb: 4096,
                strategy: "adaptive",
                force_chunked: false,
                adaptation_interval: 30,
            },

            ConfigPreset::Conservative => PresetSettings {
                memory_percentage: 5.0,
                max_memory_mb: 100,
                default_chunk_kb: 16,
                max_chunk_kb: 512,
                strategy: "static",
                force_chunked: true,
                adaptation_interval: 10,
            },
        }
    }

    /// Detect best preset based on system resources
    pub fn auto_detect() -> Self {
        if let Some(memory_stats) = memory_stats::memory_stats() {
            let total_memory_mb = memory_stats.physical_mem / (1024 * 1024);

            match total_memory_mb {
                // Very low memory systems (< 2GB)
                0..=2048 => ConfigPreset::LowMemory,

                // Low-mid memory systems (2-8GB)
                2049..=8192 => ConfigPreset::Conservative,

                // Standard systems (8-16GB)
                8193..=16384 => ConfigPreset::Balanced,

                // High memory systems (> 16GB)
                _ => ConfigPreset::HighPerformance,
            }
        } else {
            // Fallback to balanced if we can't detect
            ConfigPreset::Balanced
        }
    }
}

/// Get production configuration based on environment or auto-detection
pub fn get_system_config() -> PresetSettings {
    // Check for environment override
    if let Ok(preset_name) = std::env::var("TALE_PRESET") {
        let preset = match preset_name.to_lowercase().as_str() {
            "low" | "lowmemory" | "low_memory" => ConfigPreset::LowMemory,
            "balanced" | "balance" => ConfigPreset::Balanced,
            "high" | "highperformance" | "high_performance" => ConfigPreset::HighPerformance,
            "conservative" | "conserve" => ConfigPreset::Conservative,
            _ => {
                eprintln!("Warning: Unknown TALE_PRESET '{}', using auto-detection", preset_name);
                ConfigPreset::auto_detect()
            }
        };
        return preset.settings();
    }

    // Auto-detect based on system
    ConfigPreset::auto_detect().settings()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_sizes_are_block_aligned() {
        const BLOCK_SIZE: usize = 4096;

        let tiny = SystemDefaults::optimal_chunk_for_file(50_000);
        assert_eq!(tiny % BLOCK_SIZE, 0);
        assert_eq!(tiny, SystemDefaults::MIN_CHUNK_SIZE);

        let small = SystemDefaults::optimal_chunk_for_file(500_000);
        assert_eq!(small % BLOCK_SIZE, 0);
        assert_eq!(small, 8 * 1024);

        let medium = SystemDefaults::optimal_chunk_for_file(5_000_000);
        assert_eq!(medium % BLOCK_SIZE, 0);
        assert_eq!(medium, SystemDefaults::DEFAULT_CHUNK_SIZE);

        let large = SystemDefaults::optimal_chunk_for_file(50_000_000);
        assert_eq!(large % BLOCK_SIZE, 0);
        assert_eq!(large, 128 * 1024);

        let huge = SystemDefaults::optimal_chunk_for_file(2_000_000_000);
        assert_eq!(huge % BLOCK_SIZE, 0);
        assert_eq!(huge, 1024 * 1024);
    }

    #[test]
    fn chunking_decisions_are_good() {
        // Should not chunk tiny files
        assert!(!SystemDefaults::should_chunk_by_default(100_000));

        // Should chunk files > 1MB
        assert!(SystemDefaults::should_chunk_by_default(2_000_000));
    }

    #[test]
    fn presets_are_as_expected() {
        let low_mem = ConfigPreset::LowMemory.settings();
        assert_eq!(low_mem.memory_percentage, 5.0);
        assert_eq!(low_mem.strategy, "conservative");
        assert!(low_mem.force_chunked);

        let balanced = ConfigPreset::Balanced.settings();
        assert_eq!(balanced.memory_percentage, 10.0);
        assert_eq!(balanced.strategy, "conservative");
        assert!(!balanced.force_chunked);

        let high_perf = ConfigPreset::HighPerformance.settings();
        assert_eq!(high_perf.memory_percentage, 20.0);
        assert_eq!(high_perf.strategy, "adaptive");
        assert!(!high_perf.force_chunked);
    }

    #[test]
    fn we_have_processing_constants() {
        use super::processing::*;

        // Verify constants are reasonable
        assert_eq!(LARGE_OFFSET_THRESHOLD, 10_000);
        assert_eq!(CHUNKED_WITH_OFFSET_FILE_SIZE, 100 * 1024 * 1024);
        assert_eq!(ALWAYS_CHUNKED_FILE_SIZE, 1024 * 1024 * 1024);
        assert_eq!(ADAPTATION_INTERVAL, 20);
    }

    #[test]
    fn we_have_io_constants() {
        use super::io::*;

        // Verify reasonable defaults
        assert_eq!(TAIL_FLUSH_INTERVAL.as_millis(), 250);
        assert_eq!(FLUSH_LINE_COUNT, 40);
        assert_eq!(READ_BUFFER_SIZE, 8192);
        assert_eq!(INITIAL_CHUNK_SIZE, 32 * 1024);
    }
}
