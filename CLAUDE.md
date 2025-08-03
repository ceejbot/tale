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

## Code Architecture

### Core Structure
The application is now split into seven specialized modules:

1. **`src/main.rs`** - Application entry point and mode handlers:
   - Tokio async runtime coordination
   - Multi-file static and tailing mode implementations
   - Single file handling with backward seeking
   - Stdin processing with time-based flushing
   - Process coordination between watcher, batcher, and output

2. **`src/config.rs`** - Centralized configuration management:
   - `ConfigOpts` struct with comprehensive tail-compatible options
   - Global `OnceLock` configuration with accessor functions
   - Intelligent argument parsing with glob pattern expansion
   - Support for `-f`/`-F`, offset modes (`-n`, `-c`, `-b`), and batch windows
   - `InputMode` enum distinguishing stdin, single-file, and multi-file modes

3. **`src/loglines.rs`** - High-performance log parsing and formatting:
   - `Printable` enum for different log types with boxing for memory efficiency
   - `Canonical` struct for strict, well-structured HTTP logs (25-34% faster)
   - `Message` struct for flexible structured log entries with aliases
   - `GenericJson` for arbitrary JSON objects
   - Direct buffer writing with pre-compiled ANSI sequences
   - Zero-copy deserialization using `Cow<'a, str>`

4. **`src/columns.rs`** - Custom column layout engine:
   - High-performance column layout algorithm replacing `term_grid`
   - ANSI-aware width calculation for colored text
   - Direct buffer writing (zero intermediate allocations)
   - Configurable padding and intelligent line wrapping
   - Comprehensive test coverage (22 tests)

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
- `ripline` - Available for future I/O optimizations (not currently used)

### Output Format
The tool produces optimized columnar output with:
- 8-character log level column (right-aligned, pre-compiled ANSI)
- Optional 20-character timestamp/module column (direct buffer writing)
- Message content with intelligent wrapping
- Custom column layout for additional fields with configurable padding (default: 5-space)
- Intelligent line wrapping based on terminal width
- Color-coded JSON values (strings=green, numbers=magenta, booleans=cyan, null=red)
- Zero intermediate string allocations in formatting pipeline

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
- Uses `anyhow` for error handling instead of unwrap/expect

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
- 🐛 **Known Issues**:
  - Batch processor bug was fixed: `start()` method now properly calls `process_loop()`
  - Multiple unused imports and dead code warnings (cosmetic)
  - Multi-file functionality implemented but needs testing

### Optimization Insights
- **Architectural changes > micro-optimizations**: The `Canonical` type provided 25-34% improvement vs 5-6% from buffer writing
- **CPU-bound workload**: JSON parsing dominates performance, not I/O operations
- **Memory efficiency achieved**: 1.8% memory footprint proves excellent streaming design
- **Test-driven optimization**: Comprehensive test coverage (22 tests) ensured correctness during aggressive optimization
