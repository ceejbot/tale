# Tale Benchmark Suite

This directory contains comprehensive benchmarks for validating the performance and functionality of the `tale` log processor, with particular focus on the adaptive chunking system and memory budget management implemented in Phases 1-3.

## Available Benchmarks

### 1. `phase2_benchmark.sh` - Single vs Multi-file Strategy Testing
**Purpose**: Compare different chunking strategies and validate Phase 2 optimizations.

**What it tests**:
- Static vs Adaptive vs Conservative strategies
- Single-file vs multi-file processing
- Block-aligned chunk size effectiveness
- JSON profiling impact

**Usage**:
```bash
./scripts/phase2_benchmark.sh
./scripts/phase2_benchmark.sh --quick  # Reduced test sizes
```

**Key metrics**:
- Processing speed (lines/second)
- Strategy adaptation behavior
- Memory usage patterns
- JSON parsing variant distribution (Canonical vs Message paths)

---

### 2. `phase3_benchmarks.sh` - Multi-file Memory Budget Testing
**Purpose**: Comprehensive validation of memory budget management and multi-file processing.

**What it tests**:
- Memory pressure adaptation (Low/Moderate/High/Critical)
- Multi-file scaling (5, 10, 20 files)
- Emergency allocation scenarios
- Strategy performance under memory constraints
- Memory efficiency comparisons

**Usage**:
```bash
./scripts/phase3_benchmarks.sh
./scripts/phase3_benchmarks.sh --quick  # Faster testing
./scripts/phase3_benchmarks.sh --clean  # Cleanup only
```

**Key metrics**:
- Memory budget effectiveness
- Chunk size adaptation under pressure
- Multi-file processing scaling
- Emergency allocation behavior

---

### 3. `memory_budget_benchmark.sh` - Memory Budget Validation
**Purpose**: Focused testing of memory budget system with controlled scenarios.

**What it tests**:
- Different memory limits (5MB to 50MB)
- Memory pressure level transitions
- Strategy adaptation under memory constraints  
- Emergency allocation triggering
- Performance impact of memory management

**Usage**:
```bash
./scripts/memory_budget_benchmark.sh
```

**Key metrics**:
- Memory pressure warnings (⚠️)
- Emergency allocation messages (🆘)
- Performance degradation under constraints
- Memory usage efficiency

---

### 4. `performance_regression.sh` - Comprehensive Performance Analysis
**Purpose**: Full performance regression testing across all configurations.

**What it tests**:
- Processing mode comparisons (chunked vs standard)
- Chunking strategy performance
- Memory pressure impact on throughput
- Multi-file processing overhead
- JSON profiling overhead analysis
- Throughput analysis (MB/s, lines/sec)

**Usage**:
```bash
./scripts/performance_regression.sh
```

**Key metrics**:
- Detailed timing analysis (min/max/average)
- Throughput measurements
- Performance impact quantification
- Regression detection across configurations

---

### 5. `quick_benchmark.sh` - Fast Validation Test
**Purpose**: Quick validation that memory budget system is working correctly.

**What it tests**:
- Basic memory budget functionality
- Memory pressure warnings
- Emergency allocation behavior
- Strategy comparison
- Baseline performance comparison

**Usage**:
```bash
./scripts/quick_benchmark.sh
```

**Key metrics**:
- Quick pass/fail validation
- Memory pressure behavior
- Performance baseline comparison

## Benchmark Results Interpretation

### Memory Pressure Indicators

**Low Pressure (< 60% memory usage)**:
- ✅ Normal operation
- Full chunk sizes
- No warnings

**Moderate Pressure (60-85% memory usage)**:
- ⚠️ Memory pressure warnings may appear
- 20% chunk size reduction (0.8x factor)
- Adaptive behavior starts

**High Pressure (85-95% memory usage)**:
- ⚠️ High memory pressure warnings
- 50% chunk size reduction (0.5x factor)  
- Aggressive memory management

**Critical Pressure (> 95% memory usage)**:
- ⚠️ Critical memory pressure warnings
- 75% chunk size reduction (0.25x factor)
- Emergency measures may trigger

### Emergency Allocation Indicators

**Emergency Allocation Triggered**:
- 🆘 Emergency memory allocation messages
- Chunk size reduced to 25% of normal
- Last resort before failure

**Out of Memory**:
- ❌ Memory allocation failures
- Process termination
- Error messages about insufficient memory

## Performance Baselines

Based on previous benchmarking, expected performance characteristics:

### Small Files (< 5MB)
- **Throughput**: 300-500K lines/sec
- **Memory**: < 10MB usage
- **Strategy**: Static or Conservative recommended

### Medium Files (5-50MB)  
- **Throughput**: 200-400K lines/sec
- **Memory**: 10-50MB usage depending on strategy
- **Strategy**: Adaptive recommended for best balance

### Large Files (> 50MB)
- **Throughput**: 100-300K lines/sec  
- **Memory**: Bounded by memory budget limits
- **Strategy**: Conservative for memory efficiency, Adaptive for speed

### Multi-file Processing
- **Overhead**: ~10-20% compared to single-file
- **Scaling**: Linear up to system memory limits
- **Memory**: Multiplicative based on concurrent files

## Usage Guidelines

### For Development Testing
1. Start with `quick_benchmark.sh` for fast validation
2. Use `memory_budget_benchmark.sh` for memory-specific testing
3. Run `performance_regression.sh` before major releases

### For Performance Analysis
1. Run `phase2_benchmark.sh` to compare strategies
2. Use `phase3_benchmarks.sh` for multi-file scenarios
3. Analyze `performance_regression.sh` output for comprehensive metrics

### For Continuous Integration
1. Include `quick_benchmark.sh` in CI pipeline
2. Run full benchmarks on release candidates
3. Monitor for performance regressions

## Expected Output Examples

### Normal Operation
```bash
✅ SUCCESS
Duration: 2.450s
Lines processed: 50000
Lines/second: 20408.16
```

### Memory Pressure
```bash
⚠️ Critical memory pressure - reducing chunk size to 4096 bytes
✅ SUCCESS
Duration: 3.120s
Lines processed: 50000  
Lines/second: 16025.64
```

### Emergency Allocation
```bash
🆘 Emergency memory allocation - using 1024 byte chunks
✅ SUCCESS
Duration: 4.890s
Lines processed: 50000
Lines/second: 10224.49
```

### Memory Budget Report
```bash
Memory Budget Report:
====================
Total Limit:    10 MB
Current Usage:  8 MB (85.2%)
Peak Usage:     9 MB
Available:      2 MB
Pressure Level: High
Active Readers: 1
System Memory:  16384 MB
```

## Troubleshooting

### Benchmark Failures
- **Build errors**: Run `cargo build --release` first
- **Permission errors**: Ensure scripts are executable (`chmod +x`)
- **Timeout errors**: Increase timeout values or use `--quick` mode
- **Memory errors**: Ensure sufficient system memory available

### Performance Issues
- **Slow performance**: Check if debug build is being used accidentally
- **High memory usage**: Verify memory budget limits are appropriate
- **Inconsistent results**: Run multiple iterations and average results

### Memory Budget Issues
- **No pressure warnings**: Increase test file size or decrease memory limits
- **Emergency allocation not triggering**: Use smaller memory limits
- **Memory leaks**: Check for allocation/deallocation balance in output

## Contributing

When adding new benchmarks:
1. Follow naming convention: `{phase}_benchmark.sh`
2. Include comprehensive help documentation
3. Add entries to this README
4. Ensure cleanup of temporary files
5. Include timing and performance metrics
6. Test with both small and large datasets