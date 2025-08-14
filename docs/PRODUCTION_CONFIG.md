# Tale Production Configuration Guide

This document describes the production-ready default settings for `tale`, based on extensive benchmarking and optimization work. These defaults provide optimal balance between performance, memory efficiency, and reliability.

## Quick Start

Tale automatically configures itself based on your system resources. For most users, the defaults will work optimally without any configuration.

```bash
# Use automatic defaults (recommended)
tale myfile.log

# Override with a preset
TALE_PRESET=performance tale largefile.log

# Specify custom memory limit
tale --max-memory 104857600 myfile.log  # 100MB limit
```

## Configuration Presets

Tale includes four configuration presets that can be selected via the `TALE_PRESET` environment variable:

### 1. **Balanced** (Default for 8-16GB systems)
- **Memory**: 10% of system RAM (max 200MB)
- **Strategy**: Conservative (predictable memory usage)
- **Chunk Size**: 32KB default, up to 2MB max
- **Best For**: General purpose log processing

### 2. **Low Memory** (Default for < 2GB systems)
- **Memory**: 5% of system RAM (max 50MB)
- **Strategy**: Conservative with forced chunking
- **Chunk Size**: 16KB default, up to 256KB max
- **Best For**: Resource-constrained environments, containers

### 3. **High Performance** (Default for > 16GB systems)
- **Memory**: 20% of system RAM (max 500MB)
- **Strategy**: Adaptive (optimizes based on metrics)
- **Chunk Size**: 128KB default, up to 4MB max
- **Best For**: Large files, high-throughput requirements

### 4. **Conservative** (Default for 2-8GB systems)
- **Memory**: 5% of system RAM (max 100MB)
- **Strategy**: Static (no adaptation)
- **Chunk Size**: 16KB default, up to 512KB max
- **Best For**: Maximum predictability, shared systems

## Automatic System Detection

Tale automatically selects a preset based on available system memory:

| System Memory | Selected Preset | Memory Budget | Strategy |
|--------------|----------------|---------------|----------|
| < 2GB | Low Memory | 5% (max 50MB) | Conservative |
| 2-8GB | Conservative | 5% (max 100MB) | Static |
| 8-16GB | Balanced | 10% (max 200MB) | Conservative |
| > 16GB | High Performance | 20% (max 500MB) | Adaptive |

## Memory Management

### Memory Budget Calculation
```
Default Budget = System Memory × Preset Percentage
Final Budget = Clamp(Default Budget, 5MB min, 500MB max)
```

### Memory Pressure Levels

Tale automatically adapts chunk sizes based on memory pressure:

| Pressure Level | Memory Usage | Chunk Size Adjustment |
|---------------|--------------|----------------------|
| **Low** | < 60% | 100% (no reduction) |
| **Moderate** | 60-85% | 80% (20% reduction) |
| **High** | 85-95% | 50% (50% reduction) |
| **Critical** | > 95% | 25% (75% reduction) |

### Emergency Allocation

When memory allocation fails, Tale attempts emergency allocation at 25% of the requested size. You may see:
- `⚠️ Critical memory pressure` - High memory usage warning
- `🆘 Emergency memory allocation` - Emergency mode activated

## Chunk Size Selection

Tale automatically selects optimal chunk sizes based on file size:

| File Size | Default Chunk | Block-Aligned |
|-----------|--------------|---------------|
| < 100KB | 4KB | ✓ |
| 100KB - 1MB | 8KB | ✓ |
| 1MB - 10MB | 32KB | ✓ |
| 10MB - 100MB | 128KB | ✓ |
| 100MB - 1GB | 512KB | ✓ |
| > 1GB | 1MB | ✓ |

All chunk sizes are aligned to 4KB filesystem blocks for optimal I/O performance.

## Chunking Strategy

### When Chunking is Used

By default, chunking is automatically enabled for files > 1MB. This can be controlled:

```bash
# Force chunking for all files
tale --chunked myfile.log

# Disable chunking (not recommended for large files)
tale --no-chunked myfile.log

# Automatic (recommended)
tale myfile.log
```

### Strategy Selection

Three strategies are available:

1. **Conservative** (Default)
   - Predictable memory usage
   - Gradual adaptation
   - Best for production workloads

2. **Adaptive**
   - Dynamic optimization based on metrics
   - Faster processing for uniform data
   - Higher memory variance

3. **Static**
   - Fixed chunk sizes
   - Most predictable
   - No runtime adaptation

Override strategy:
```bash
tale --chunk-strategy adaptive myfile.log
```

## Performance Expectations

Based on benchmarking with production defaults:

### Small Files (< 5MB)
- **Throughput**: 300-500K lines/sec
- **Memory**: < 10MB
- **Latency**: < 100ms startup

### Medium Files (5-50MB)
- **Throughput**: 200-400K lines/sec
- **Memory**: 10-50MB (bounded by budget)
- **Latency**: < 200ms startup

### Large Files (> 50MB)
- **Throughput**: 100-300K lines/sec
- **Memory**: Bounded by budget (no OOM)
- **Latency**: < 500ms startup

### Multi-file Processing
- **Overhead**: ~10-20% vs single file
- **Scaling**: Linear with file count
- **Memory**: Shared budget across files

## Environment Variables

### `TALE_PRESET`
Select a configuration preset:
```bash
export TALE_PRESET=performance  # or: low, lowmemory, conservative, balanced
```

### System Resource Detection
Tale respects standard resource limits:
- `ulimit` settings
- Container memory limits
- cgroup restrictions

## Command-line Options

Override any default with command-line flags:

```bash
# Memory limit (bytes)
tale --max-memory 52428800 file.log  # 50MB

# Chunking strategy
tale --chunk-strategy adaptive file.log

# Force/disable chunking
tale --chunked file.log
tale --no-chunked file.log

# Batch window for multi-file (ms)
tale --window 500 *.log
```

## Monitoring

### Debug Builds
In debug builds, additional profiling is available:
```bash
# JSON parsing profiling
cargo run -- --profile-json file.log

# Conservative memory mode
cargo run -- --conservative file.log
```

### Production Monitoring
- Memory usage stays within configured budget
- No OOM crashes even with large files
- Predictable performance degradation under pressure
- Clear warning messages for resource constraints

## Best Practices

1. **Use defaults**: The automatic configuration works well for most cases
2. **Monitor warnings**: Pay attention to memory pressure warnings
3. **Test with your data**: Run benchmarks with representative files
4. **Set explicit limits**: In production, consider setting explicit memory limits
5. **Use presets**: For known workloads, select appropriate preset

## Troubleshooting

### High Memory Usage
- Check if chunking is enabled (should be for files > 1MB)
- Verify memory budget is appropriate for workload
- Consider Conservative or Low Memory preset

### Slow Performance
- Check for memory pressure warnings
- Try High Performance preset for large files
- Ensure release build is used (`--release`)

### Emergency Allocations
- Increase memory budget
- Use smaller chunk strategy
- Consider processing files sequentially

## Examples

### Container with Limited Memory
```bash
# In a 512MB container
TALE_PRESET=low tale /var/log/app.log
```

### Large File Processing
```bash
# Process 1GB+ files efficiently
TALE_PRESET=performance tale --max-memory 209715200 huge.log
```

### Multi-file Tailing
```bash
# Tail multiple files with controlled memory
tale -f --window 500 --max-memory 104857600 *.log
```

### Resource-Constrained System
```bash
# Minimal resource usage
TALE_PRESET=conservative tale --chunk-strategy static small.log
```

## Summary

Tale's production defaults provide:
- **Automatic configuration** based on system resources
- **Memory safety** through budget management
- **Predictable performance** with conservative defaults
- **Adaptive behavior** when resources allow
- **Clear feedback** through warnings and errors

For most users, simply running `tale` without options will provide optimal performance with memory safety guarantees.