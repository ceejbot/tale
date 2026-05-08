//! This strategy picks an optimal chunk size at the start of reading
//! and then doesn't vary it but chugs right on through. This is fine for
//! single-file reading.

#[derive(Debug, Clone)]
pub struct StaticStrategy {
    pub chunk_size: usize,
    pub config: ChunkConfig,
}

impl StaticStrategy {
    pub fn optimal_for_file(file_size: u64) -> Self {
        Self {
            chunk_size: optimal_chunk_size(file_size),
            config: ChunkConfig::default(),
        }
    }

    /// the fixed chunk size we should use
    pub fn initial_chunk_size(&self) -> usize {
        self.chunk_size
    }
}

impl Default for StaticStrategy {
    fn default() -> Self {
        // Use production default chunk size
        let default_size = crate::defaults::SystemDefaults::DEFAULT_CHUNK_SIZE;
        let aligned_size = align_to_block_size(default_size, get_optimal_block_size());
        Self {
            chunk_size: aligned_size,
            config: ChunkConfig::default(),
        }
    }
}

/// Configuration for FileChunk processing
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Maximum overlap between chunks to handle line boundaries
    pub overlap_size: usize,
    /// Whether to use memory-constrained processing
    pub low_memory_mode: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            overlap_size: 1024, // 1KB overlap for line boundaries
            low_memory_mode: false,
        }
    }
}

/// Align chunk size to filesystem block boundaries for optimal I/O
pub fn align_to_block_size(size: usize, block_size: usize) -> usize {
    // Round up to nearest block boundary
    size.div_ceil(block_size) * block_size
}

/// Get optimal filesystem block size based on typical modern filesystems
pub fn get_optimal_block_size() -> usize {
    // Most modern filesystems use 4KB blocks
    4096
}

/// Determine optimal chunk size based on file characteristics
pub fn optimal_chunk_size(file_size: u64) -> usize {
    crate::defaults::SystemDefaults::optimal_chunk_for_file(file_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_to_blocks() {
        let block_size = 4096;

        // Test exact alignment
        assert_eq!(align_to_block_size(4096, block_size), 4096);
        assert_eq!(align_to_block_size(8192, block_size), 8192);

        // Test rounding up
        assert_eq!(align_to_block_size(4097, block_size), 8192);
        assert_eq!(align_to_block_size(1000, block_size), 4096);
        assert_eq!(align_to_block_size(9000, block_size), 12288);

        // Test zero and small values
        assert_eq!(align_to_block_size(0, block_size), 0);
        assert_eq!(align_to_block_size(1, block_size), 4096);
    }

    #[test]
    fn optimal_chunk_sizes_are_aligned() {
        let block_size = get_optimal_block_size();

        // Test different file sizes
        let small_chunk = optimal_chunk_size(500_000);
        let medium_chunk = optimal_chunk_size(50_000_000);
        let large_chunk = optimal_chunk_size(500_000_000);

        // All should be aligned to block boundaries
        assert_eq!(small_chunk % block_size, 0);
        assert_eq!(medium_chunk % block_size, 0);
        assert_eq!(large_chunk % block_size, 0);

        println!("Block-aligned chunk sizes:");
        println!("  Small file (500KB): {} bytes", small_chunk);
        println!("  Medium file (50MB): {} bytes", medium_chunk);
        println!("  Large file (500MB): {} bytes", large_chunk);
    }
}
