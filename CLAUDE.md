# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`tale` is a high-performance command-line utility for pretty-printing newline-delimited JSON (ndjson) log files. It's a single-binary Rust application that reads structured log data and formats it with colors, custom column layout, and terminal-aware wrapping. The tool has been heavily optimized for speed and memory efficiency.

## Development Commands

### Building and Running
- `cargo build` - Build the project
- `cargo build --release` - Build optimized release version
- `cargo run` - Run the application (reads from stdin by default)
- `cargo run -- <file>` - Run with a specific file
- `cargo run -- <file1> <file2>` - Run with multiple files (static mode)
- `cargo run -- -f <file1> <file2>` - Follow multiple files (tailing mode)
- `cargo run -- -f *.log` - Follow all .log files using glob patterns
- `cargo run -- -n 10 <file>` - Show last 10 lines
- `cargo run -- --window 500 -f *.log` - Follow with 500ms batch window
- `cargo run -- --help` - Show command-line help

### Testing and Quality
- `cargo test` - Run all tests
- `cargo clippy` - Run linter (configured to deny `unwrap_used`)
- `cargo fmt` - Format code
- `cargo bench` - Run performance benchmarks using Criterion

### Benchmarking
- `cargo bench --bench chunking_strategies` - Chunk size calculation benchmarks
- `cargo bench --bench memory_management` - Memory budget and pressure tests
- `cargo bench --bench file_processing` - File processor performance comparison
- `scripts/benchmark.sh` - Comprehensive shell-based benchmarks

### Tools for shell interactions

Install via homebrew if these are missing.

- Is it about finding FILES? use `fd`
- Is it about finding TEXT/strings? use `rg`
- Is it about finding CODE STRUCTURE? use `ast-grep`
- Is it about SELECTING from multiple results? pipe to `fzf`
- Is it about interacting with JSON? use `jq`
- Is it about interacting with YAML or XML? use `yq`
- Is it about interacting with TOML? use `tomato`
- Is it about FINDING and REPLACING text by pattern in files? use `sd`

## Code Architecture

### Core Structure
The application is organized into specialized modules and a readers subsystem:

1. **`src/main.rs`** - Application entry point and mode handlers:
   - Tokio async runtime coordination
   - Multi-file static and tailing mode implementations
   - Command-line argument parsing with clap
   - Process coordination between components

2. **`src/config.rs`** - Centralized configuration management:
   - `ConfigOpts` struct with comprehensive tail-compatible options
   - Global `OnceLock` configuration with accessor functions
   - Intelligent argument parsing with glob pattern expansion
   - Support for `-f`/`-F`, offset modes (`-n`, `-c`, `-b`), and batch windows
   - `InputMode` enum distinguishing stdin, single-file, and multi-file modes

3. **`src/logpatterns/`** - Log parsing and formatting subsystem:
   - `patterns.rs` - `Printable` enum for different log types with memory-efficient boxing
   - `Canonical` struct for strict HTTP logs (25-34% faster)
   - `Message` struct for flexible structured log entries with aliases
   - `formatting.rs` - Direct buffer writing with pre-compiled ANSI sequences
   - `columns.rs` - Custom column layout engine with ANSI-aware width calculation
   - `sourced.rs` - Wrapper for multi-file source tracking

4. **`src/readers/`** - File processing abstraction layer:
   - `mod.rs` - `FileProcessor` trait and processor selection logic
   - `buffered.rs` - `BufferedFileProcessor` for small files with forward-only reading
   - `chunked.rs` - `ChunkedFileReader` for memory-efficient large file processing
   - `backseeking.rs` - `BackSeekingProcessor` for tail-like backward seeking (handles negative offsets)
   - `stdin.rs` - `StdinProcessor` for consolidated stdin handling with offset support
   - `strategies/` - `StaticStrategy` for file-size-based chunk sizing
   - Smart processor selection based on file size and offset requirements

5. **`src/file_state.rs`** - File state tracking for multi-file tailing:
   - Individual file position tracking with inode-based rotation detection
   - `FileStateManager` for coordinating multiple file states
   - Support for sticky (`-F`) vs follow (`-f`) semantics
   - Efficient new-line reading from specific file positions

6. **`src/watcher.rs`** - File system event monitoring:
   - Cross-platform file watching using `notify` crate
   - Async event conversion and coordination via tokio channels
   - Integration with file state manager for change detection
   - Support for multiple file watching with event aggregation

7. **`src/batch.rs`** - Multi-file line batching and timestamp sorting:
   - Time-windowed batching for chronological log line ordering
   - Priority queue-based sorting by extracted timestamps
   - Async processing pipeline with configurable batch windows
   - Support for mixed timestamped/non-timestamped log lines

8. **`src/errors.rs`** - Rich error handling with thiserror + miette:
   - Comprehensive error types with diagnostic information
   - File errors with similarity suggestions (edit distance algorithm)
   - JSON errors with source location tracking
   - I/O errors with proper context
   - Color-coded error messages using `owo-colors`

9. **`src/metrics/`** - Performance monitoring:
   - `collector.rs` - `ChunkMetrics` for basic performance tracking (chunks seen, bytes, duration)
   - `memory.rs` - System memory detection and pressure monitoring
   - Cross-platform memory statistics integration

10. **`src/memory_budget.rs`** - Memory allocation management:
    - `MemoryBudget` for tracking and limiting memory usage
    - Four-level pressure detection (Low, Moderate, High, Critical)
    - Allocation tracking with automatic cleanup on drop
    - Memory statistics and reporting

### Key Data Structures

**Canonical struct** - Optimized for well-structured HTTP logs (fastest path):
- Required: `timestamp`, `level`, `message`, `method`, `url`, `status`, `elapsed`, `size`, `request_id`, `remote_host`, `user_agent`
- All fields use `Cow<'a, str>` for zero-copy deserialization
- Provides 25-34% performance improvement over flexible parsing
- Strict typing (e.g., `status: usize`, `size: usize`)

**Message struct** - Flexible structured log entries with field aliases:
- Required: `message` (alias: `msg`) and `level` (alias: `lvl`, `severity`)
- Optional: `timestamp` (alias: `time`, `ts`)
- Optional HTTP fields: `method`, `url`, `status` (alias: `statusCode`, `code`), `elapsed` (alias: `elapsed_ms`, `elapsed_time`), `size` (multiple aliases)
- Optional metadata: `request_id` (alias: `requestId`), `host` (alias: `hostname`, `remote_host`), `module` (alias: `mod`, `lib`), `file`, `line`
- All string fields use `Cow<'a, str>` for zero-copy when possible
- Flexible JSON handling via `#[serde(flatten)]` for additional fields

**Printable enum** - Memory-optimized with boxed variants:
- `Canonical(Box<Canonical<'a>>)` - Fastest path for structured HTTP logs
- `Message(Box<Message<'a>>)` - Flexible log parsing
- `Json(GenericJson)` - Generic JSON objects
- Enum size optimized from 360 → ~40 bytes via boxing

**BackSeekingProcessor struct** - Handles backward seeking and tail-like behavior:
- Specialized for negative offsets and byte-based operations
- Efficient backward line seeking without loading entire file
- Memory-bounded approach for large files
- Supports tail offset modes: lines and bytes (positive and negative)
- `-b`/`--blocks` is accepted for `tail` compatibility but ignored
- Primary processor for traditional tail functionality

**StdinProcessor struct** - Consolidated stdin processing patterns:
- Encapsulates stdin/stdout locks, line buffers, and formatting pipeline
- Methods: `skip_lines()`, `skip_bytes()`, `backtrack_lines()`, `backtrack_bytes()`, `tail()`
- Automatic flushing and batch processing (40 lines per flush)
- Smart overshoot handling for byte-based operations with partial line continuation
- Eliminates ~80 lines of duplicate code across offset implementations

**CircularByteBuffer struct** - Memory-bounded byte operations:
- Fixed-size circular buffer for last-N-bytes operations
- Write-through design with exact capacity limits
- Efficient wraparound logic for reading last N bytes
- Used by negative byte and block offset implementations

### Key Dependencies
- `clap` - Command-line argument parsing with derive macros and custom styling
- `serde_json` - JSON parsing with flexible field aliases and `Cow<'a, str>` support
- `jiff` - Timestamp handling and formatting
- `owo-colors` - Terminal colors and styling
- `textwrap` - Terminal width detection
- `ansi-width` - ANSI escape sequence aware width calculation for colored text
- `bytes` - Efficient byte buffer handling with `BytesMut`
- `humansize` - Binary size formatting (KB, MB, GB)
- `tokio` - Async runtime for multi-file coordination and event handling
- `notify` - Cross-platform file system event watching
- `async-watcher` - Async wrapper for notify integration
- `futures` - Stream utilities for async coordination
- `glob` - Glob pattern matching for file expansion
- `thiserror` - Ergonomic error type definitions
- `miette` - Rich error diagnostics with source location tracking
- `criterion` - Statistical benchmarking framework for performance testing
- `ripline` - Available for future I/O optimizations (not currently used)

### Output Format
The tool produces optimized columnar output with:
- 8-character log level column (right-aligned, pre-compiled ANSI)
- Optional 20-character timestamp/module column (direct buffer writing)
- Message content with intelligent wrapping
- Custom column layout for additional fields with configurable padding (default: 5-space)
- Intelligent line wrapping based on terminal width
- Uniform colorization via `colorize_map_entry()`: keys=dimmed, strings=green (quoted), numbers=magenta, booleans=cyan, null=red
- All log types (Canonical, Message, Java, GenericJson) use the same color scheme

### I/O Performance Characteristics
- **Memory Usage**: 4.2MB peak for 235MB files (1.8% memory footprint)
- **Throughput**: 387K lines/sec on large files (1.1M lines)
- **I/O Pattern**: CPU-bound (74% CPU, 26% I/O) - JSON processing is the bottleneck
- **File Support**: stdin, single files, with offset support (positive/negative)
- **Seeking**: Efficient backward seeking for tail functionality (e.g., `-10` for last 10 lines)
- **Buffering**: Optimized buffered I/O with time-based flushing for stdin tailing
- **Scalability**: Constant memory usage regardless of file size

### Linting Configuration
Strict linting is enforced:
- `unsafe_code = "deny"`
- `unwrap_used = "deny"` (Clippy)
- Additional rust lints: `future_incompatible = "deny"`, `trivial_casts = "warn"`, `rust_2018_idioms = "warn"`
- Uses `thiserror` + `miette` for rich error handling with diagnostics

### Performance Optimizations Applied

**Major optimizations that significantly improved performance:**

1. **Canonical Log Type** (25-34% improvement):
   - Strict deserialization for well-structured HTTP logs
   - Required fields eliminate `Option<>` branching overhead
   - Specialized formatting path optimized for common use cases

2. **Zero-Copy Deserialization** (reduced ~1.8M allocations/sec):
   - `Cow<'a, str>` for all string fields
   - `#[serde(borrow)]` annotations enable borrowing from input buffer
   - Eliminates string allocations when data can be borrowed

3. **Custom Column Layout Engine** (replaced `term_grid`):
   - Direct buffer writing with zero intermediate allocations
   - Intelligent line wrapping with configurable padding
   - ANSI-aware width calculation for colored text
   - 22 comprehensive tests ensure correctness

4. **Direct Buffer Writing** (5-6% improvement):
   - Pre-compiled ANSI escape sequences for log levels
   - Eliminated `format!` macro calls in hot paths
   - Direct `BytesMut` buffer operations

5. **Memory Layout Optimization**:
   - Boxed enum variants: `Printable` size reduced from 360 → ~40 bytes
   - Better CPU cache performance
   - Reduced stack allocation pressure

**Overall Performance Improvement**: 28-37% faster processing (616ms → 443ms on 23MB files)

### Current State
The application is highly optimized and fully functional with:
- ✅ **Simplified architecture**: Clean chunk sizing and memory management
  - StaticStrategy for file-size-based chunk sizing (single strategy, no over-engineering)
  - Memory pressure detection and response
  - Comprehensive benchmark suite with Criterion
- ✅ **Complete stdin offset support** matching `tail` behavior:
  - Positive offsets: `-n +N`, `-c +N` (skip first N units)
  - Negative offsets: `-n -N`, `-c -N` (show last N units)
  - `-b`/`--blocks` accepted for compatibility but ignored
  - All offset modes work with stdin and files
  - Memory-bounded circular buffers for bytes
  - Adaptive VecDeque for lines with 10MB memory limit
- ✅ **StdinProcessor refactor** (completed 2025-01-08):
  - All stdin processing consolidated into clean `StdinProcessor` struct
  - Methods: `skip_lines()`, `skip_bytes()`, `backtrack_lines()`, `backtrack_bytes()`, `tail()`
  - Smart overshoot handling with byte-based parsing (not string-based)
  - Eliminated ~80 lines of duplicate code
  - Clean `handle_stdin()` delegation with intuitive API
- ✅ High-performance stdin processing with tailing support
- ✅ File reading with offset support and backward seeking
- ✅ Zero-copy JSON parsing with multiple format support
- ✅ Custom column layout engine with comprehensive test coverage
- ✅ Excellent memory efficiency (1.8% memory footprint)
- ✅ Direct buffer writing throughout formatting pipeline
- ✅ ANSI-aware terminal output with intelligent wrapping
- ✅ **Multi-file tailing functionality** with async coordination:
  - Cross-platform file watching via `notify` crate
  - Timestamp-based line batching and chronological sorting
  - Configurable batch windows (default: 250ms)
  - Glob pattern support for file matching
  - Inode-based file rotation detection
  - Support for both static (read-once) and tailing modes
- ✅ **Production Ready**: All major functionality complete and tested
  - Comprehensive test suite (90 tests passing)
  - Multi-file functionality thoroughly validated
  - Performance benchmarks demonstrate significant improvements
  - Memory management handles resource constraints gracefully

### Optimization Insights
- **Architectural changes > micro-optimizations**: The `Canonical` type provided 25-34% improvement vs 5-6% from buffer writing
- **CPU-bound workload**: JSON parsing dominates performance, not I/O operations
- **Memory efficiency achieved**: 1.8% memory footprint proves excellent streaming design
- **Test-driven optimization**: Comprehensive test coverage (40 tests) ensured correctness during aggressive optimization
- **Refactoring value**: `StdinProcessor` consolidation eliminated duplication while improving maintainability

## Development Status

### ✅ Completed Major Features
1. **Simplified Architecture**: Static chunk sizing, streamlined memory management
2. **Multi-file Processing**: Static and tailing modes with glob pattern support
3. **Memory Management**: Budget allocation, pressure detection, and graceful degradation
4. **Performance Optimization**: 28-37% faster processing with zero-copy deserialization
5. **Uniform Colorization**: All log types use `colorize_map_entry()` as single source of truth
6. **Comprehensive Testing**: 90 tests passing with benchmark suite
7. **Rich Error Handling**: Diagnostic messages with helpful suggestions

### Future Enhancements (Optional)
1. **Chunk Pooling**: Vec<u8> recycling for high-throughput scenarios (5-15% improvement)
2. **Time-based Offsets**: Advanced log analysis with timestamp-based seeking
3. **Format Extensions**: Support for other structured log formats (logfmt, etc.)
4. **Layout Redesign**: Improved column packing and visual layout for field display

### Maintenance Tasks
1. **Documentation**: Keep examples and guides updated with new features
2. **Performance Monitoring**: Regular benchmarking to prevent regressions  
3. **Cross-platform Testing**: Validate functionality across different systems
4. **Dependency Updates**: Keep dependencies current for security and performance

## Recent Work

### 2026-02-21: Complexity Cleanup

**Removed over-engineered strategies** (~700 lines deleted):
- Deleted AdaptiveStrategy (10% slower than static in benchmarks) and ConservativeStrategy (never triggered)
- Collapsed Strategy enum to just StaticStrategy used directly
- Removed IsStrategy trait, strategy CLI flags, strategy selection logic

**Simplified memory management** (~400 lines simplified):
- Removed per-reader tracking (ReaderMemoryStats, HashMap allocation accounting)
- Removed MovingAverage, Trend, MetricsCollector from metrics
- Simplified ChunkMetrics to just chunks_seen/total_bytes/total_duration
- Removed `is_memory_constrained()` dead code
- MemoryBudget simplified to basic limit/usage/peak tracking

**Unified colorization**:
- All log types (Canonical, Message, Java, GenericJson) now use `colorize_map_entry()` as single source of truth
- Removed per-field semantic coloring (HTTP=blue, k8s=cyan, performance=purple, etc.)
- Uniform scheme: keys=dimmed, strings=green (quoted), numbers=magenta, bools=cyan, null=red
- Canonical and Java converted from inline buffer rendering to cells/columns system
- Removed dead code: `start_new_line()`, unused imports

**Net result**: ~1100 lines of over-engineered code removed, zero functionality lost, all 90 tests passing
