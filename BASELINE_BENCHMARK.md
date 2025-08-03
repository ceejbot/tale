# Tale Baseline Benchmark Results

**Date:** August 2, 2025  
**Version:** Current implementation (before allocation optimizations)  
**Platform:** macOS (Apple Silicon)  
**Rust Version:** 2024 edition  

## Test Dataset

| File | Lines | Size | Description |
|------|-------|------|-------------|
| `fixtures/benchmarks/small.log` | 1,161 | 241KB | Quick validation |
| `fixtures/benchmarks/medium.log` | 111,872 | 23MB | Primary benchmark |
| `fixtures/benchmarks/large.log` | 1,116,368 | 235MB | Stress test |

**Data Mix:**
- 40% HTTP request logs (JSON)
- 30% Application logs (JSON) 
- 10% Database logs (JSON)
- 10% Plain text logs (stack traces, etc.)
- 10% Malformed JSON (error cases)

## Baseline Performance Results

### Processing Rates (File Input)

| Dataset | Lines/sec | MB/sec | Total Time |
|---------|-----------|--------|------------|
| Small | 7,241 | 6.23 | ~0.16s |
| Medium | 187,377 | 40.19 | ~0.60s |
| Large | 243,298 | 52.52 | ~4.6s |

### Detailed Throughput (hyperfine)

**Medium file (111K lines, 23MB):**
```
Time (mean ± σ):     616.2 ms ±   3.8 ms    [User: 404.7 ms, System: 83.3 ms]
Range (min … max):   609.8 ms … 619.4 ms    5 runs
```

**Key Metrics:**
- **Throughput:** ~181,000 lines/sec, ~37 MB/sec
- **CPU Time:** 404.7ms user + 83.3ms system = 488ms total
- **I/O Overhead:** 128ms (616ms wall - 488ms CPU)
- **Variance:** Very low (±3.8ms), indicating consistent performance

## Known Issues

1. **Output Difference:** File vs stdin processing produces slightly different output (extra blank lines in stdin mode)
2. **Memory Usage:** Not yet measured in detail
3. **Allocation Patterns:** Not yet profiled

## Optimization Targets

Based on these baseline results, optimization efforts should focus on:

1. **High-frequency allocations** in JSON deserialization path
2. **String formatting** in the non-JSON error path  
3. **Buffer management** in stdin processing
4. **Memory usage** profiling for large files

## Next Steps

1. Profile memory usage and allocation patterns
2. Fix stdin/file output consistency issue
3. Implement allocation reduction optimizations
4. Re-benchmark and compare improvements

---

*This baseline will be used to measure the impact of allocation reduction optimizations.*