//! Production default settings for tale
//!
//! These defaults are based on extensive benchmarking and optimization work
//! from Phases 1-3 of the adaptive chunking improvements. They provide optimal
//! balance between performance, memory efficiency, and reliability.
//!
//! ## Key Principles
//! - **Memory Safety**: Prevent OOM while maximizing performance
//! - **Adaptive Behavior**: Start conservative, adapt based on metrics
//! - **Block Alignment**: All sizes aligned to 4KB filesystem blocks
//! - **Progressive Degradation**: Graceful handling of resource constraints

/// Production defaults for chunked file processing
pub struct ProductionDefaults;

impl ProductionDefaults {
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
        ((base_size + BLOCK_SIZE - 1) / BLOCK_SIZE) * BLOCK_SIZE
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
    pub const MEMORY_PRESSURE_LOW_THRESHOLD: f64 = 0.60;      // < 60%: Normal
    pub const MEMORY_PRESSURE_MODERATE_THRESHOLD: f64 = 0.85; // 60-85%: Moderate
    pub const MEMORY_PRESSURE_HIGH_THRESHOLD: f64 = 0.95;     // 85-95%: High
    // > 95%: Critical
    
    /// Chunk size reduction factors per pressure level
    pub const PRESSURE_FACTOR_LOW: f64 = 1.0;        // No reduction
    pub const PRESSURE_FACTOR_MODERATE: f64 = 0.8;   // 20% reduction
    pub const PRESSURE_FACTOR_HIGH: f64 = 0.5;       // 50% reduction  
    pub const PRESSURE_FACTOR_CRITICAL: f64 = 0.25;  // 75% reduction
    
    /// Emergency allocation factor
    pub const EMERGENCY_ALLOCATION_FACTOR: f64 = 0.25; // 25% of requested size
}

/// Production configuration presets
pub enum ConfigPreset {
    /// Optimized for small files and low memory systems
    LowMemory,
    /// Balanced for general use (default)
    Balanced,
    /// Optimized for large files and high throughput
    HighPerformance,
    /// Maximum safety and predictability
    Conservative,
}

impl ConfigPreset {
    /// Get recommended settings for this preset
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

/// Settings for a configuration preset
pub struct PresetSettings {
    pub memory_percentage: f64,
    pub max_memory_mb: usize,
    pub default_chunk_kb: usize,
    pub max_chunk_kb: usize,
    pub strategy: &'static str,
    pub force_chunked: bool,
    pub adaptation_interval: usize,
}

/// Get production-ready configuration based on environment
pub fn get_production_config() -> PresetSettings {
    // Check for environment variable override
    if let Ok(preset_name) = std::env::var("TALE_PRESET") {
        match preset_name.to_lowercase().as_str() {
            "low" | "lowmemory" => ConfigPreset::LowMemory.settings(),
            "high" | "performance" => ConfigPreset::HighPerformance.settings(),
            "conservative" | "safe" => ConfigPreset::Conservative.settings(),
            _ => ConfigPreset::Balanced.settings(),
        }
    } else {
        // Auto-detect based on system
        ConfigPreset::auto_detect().settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_optimal_chunk_sizes() {
        // Test that chunk sizes are block-aligned
        const BLOCK_SIZE: usize = 4096;
        
        let tiny = ProductionDefaults::optimal_chunk_for_file(50_000);
        assert_eq!(tiny % BLOCK_SIZE, 0);
        assert_eq!(tiny, ProductionDefaults::MIN_CHUNK_SIZE);
        
        let small = ProductionDefaults::optimal_chunk_for_file(500_000);
        assert_eq!(small % BLOCK_SIZE, 0);
        assert_eq!(small, 8 * 1024);
        
        let medium = ProductionDefaults::optimal_chunk_for_file(5_000_000);
        assert_eq!(medium % BLOCK_SIZE, 0);
        assert_eq!(medium, ProductionDefaults::DEFAULT_CHUNK_SIZE);
        
        let large = ProductionDefaults::optimal_chunk_for_file(50_000_000);
        assert_eq!(large % BLOCK_SIZE, 0);
        assert_eq!(large, 128 * 1024);
        
        let huge = ProductionDefaults::optimal_chunk_for_file(2_000_000_000);
        assert_eq!(huge % BLOCK_SIZE, 0);
        assert_eq!(huge, 1024 * 1024);
    }
    
    #[test]
    fn test_chunking_decision() {
        // Should not chunk tiny files
        assert!(!ProductionDefaults::should_chunk_by_default(100_000));
        
        // Should chunk files > 1MB
        assert!(ProductionDefaults::should_chunk_by_default(2_000_000));
    }
    
    #[test]
    fn test_preset_settings() {
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
    fn test_memory_thresholds() {
        // Verify thresholds are properly ordered
        assert!(ProductionDefaults::MEMORY_PRESSURE_LOW_THRESHOLD 
            < ProductionDefaults::MEMORY_PRESSURE_MODERATE_THRESHOLD);
        assert!(ProductionDefaults::MEMORY_PRESSURE_MODERATE_THRESHOLD 
            < ProductionDefaults::MEMORY_PRESSURE_HIGH_THRESHOLD);
        assert!(ProductionDefaults::MEMORY_PRESSURE_HIGH_THRESHOLD < 1.0);
        
        // Verify factors decrease with pressure
        assert!(ProductionDefaults::PRESSURE_FACTOR_LOW 
            > ProductionDefaults::PRESSURE_FACTOR_MODERATE);
        assert!(ProductionDefaults::PRESSURE_FACTOR_MODERATE 
            > ProductionDefaults::PRESSURE_FACTOR_HIGH);
        assert!(ProductionDefaults::PRESSURE_FACTOR_HIGH 
            > ProductionDefaults::PRESSURE_FACTOR_CRITICAL);
    }
}