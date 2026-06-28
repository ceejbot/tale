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

The crate is split into a binary (`src/main.rs`) and a library (`src/lib.rs`).
The binary owns CLI parsing; the library owns everything else and stays free of
clap so it isn't a polluted dependency for any future library consumer.

### Top-level files
- **`src/main.rs`** — binary entry point. Owns the clap-derived `Args` struct,
  implements `tale_ndjson::config::CliOptions` for it, runs the tokio runtime,
  and dispatches to one of `readers::handle_stdin`, `readers::handle_file`, or
  `multiplexed::handle_static` / `multiplexed::handle_tailing` based on
  `InputMode`.
- **`src/lib.rs`** — library root. Re-exports the small public surface
  (`TaleError`, `MemoryBudget`, `MemoryPressure`, `StaticStrategy`,
  `ChunkedFileReader`, `FileProcessor`) and exposes only the modules consumers
  actually need (`config`, `metrics`, `multiplexed`, `readers`). Provides
  `process_line` and `strip_line_ending` helpers.
- **`src/config.rs`** — `ConfigOpts` struct, the `CliOptions` trait that
  decouples the lib from clap, the `OnceLock` (production) /
  `RefCell` (tests) runtime split for global config, and accessor functions.
  Handles glob expansion and positional-args parsing.
- **`src/defaults.rs`** *(crate-private)* — chunk-size constants, memory
  pressure thresholds, system memory presets (`LowMemory`, `Balanced`, …),
  and `optimal_chunk_for_file`.
- **`src/errors.rs`** *(crate-private)* — `TaleError` and friends
  (`FileError`, `JsonError`, `ConfigError`, `IoError`) built with `thiserror`
  + `miette`. Includes edit-distance "did you mean?" suggestions for ENOENT.
- **`src/json_profiler.rs`** *(debug-only)* — atomic counters tracking which
  `Printable` variant each line parses into. Compiled in only when
  `cfg(debug_assertions)`; excluded from release builds entirely.
- **`src/memory_budget.rs`** *(crate-private)* — `MemoryBudget` with
  four-level pressure (`Low`/`Moderate`/`High`/`Critical`) and RAII allocation
  tracking. Re-exported at the lib root.

### `src/logpatterns/` — log parsing and formatting

- **`mod.rs`** — `PrettyPrintable` trait (with default `fmt_pretty` method)
  and the `Printable<'a>` enum. The enum is `#[serde(untagged)]` and ordered
  most-specific to least-specific; deserialization tries each variant in
  turn. Variants are boxed where the inner type is large.
- **`patterns.rs`** — the variant types: `Canonical`, `Java`, `Message`,
  `Timestamped`, `GenericJson`. `Canonical` is the strictly-typed fast path
  for Stripe-style HTTP logs. `Message` is the flexible superset (handles
  nginx, k8s, GCP, Docker, OpenTelemetry via field aliases). Includes
  `de_string_or_number` for fields like `status` and `size` that real
  producers emit either as strings or as numbers.
- **`logfmt.rs`** — `LogfmtLine` for Heroku-style `key=value` lines.
- **`formatting.rs`** — `LayoutMetrics` (one place that knows the column
  geometry), `pad_spaces`, `write_cells_with_padding`, level/timestamp
  column writers, and the unified `colorize_map_entry` /
  `colorize_json_value` helpers. Each `Printable` variant uses
  `LayoutMetrics::current()` rather than recomputing layout dimensions
  inline.
- **`columns.rs`** — elastic-tabstop column layout (uses `tabwriter`),
  ANSI-aware width calculation.
- **`sourced.rs`** — `SourcedLine<'a>` wraps a `Printable` with a path and
  line number; renders `==> filename <==` headers in multi-file mode.

### `src/readers/` — file processing

- **`mod.rs`** — `FileProcessor` trait, `FileProcessorType<'a>` enum
  dispatching across the four implementations, and `create_file_processor`
  which picks the right processor based on file size and offset shape.
  Also contains `handle_file` and `wait_for_file_creation` (sticky mode).
- **`buffered.rs`** — `BufferedFileProcessor` for small files
  (forward-only `BufRead`).
- **`chunked.rs`** — `ChunkedFileReader` for large files with
  bounded-memory chunked reads and pending-data carryover for
  cross-chunk lines.
- **`backseeking.rs`** — `BackSeekingProcessor` for negative offsets,
  byte offsets, and tailing. Walks backward from EOF in 8KB chunks until
  the requested number of newlines is found.
- **`stdin.rs`** — `StdinProcessor` for stdin with all the offset modes
  (`skip_lines`, `skip_bytes`, `backtrack_lines`, `backtrack_bytes`,
  `tail`). Includes `CircularByteBuffer` for last-N-bytes operations and
  a temp-file fallback for `backtrack_lines` past the 10MB memory budget.
- **`strategies/`** — `StaticStrategy` and `ChunkConfig`: file-size-aware
  but otherwise static chunk sizing.

### `src/multiplexed/` — multi-file static + tailing

- **`mod.rs`** — `handle_static` (read all files once, sort
  chronologically, emit) and `handle_tailing` (watch for changes, batch,
  emit). Both route through `SourcedLine` so file-name headers work
  consistently.
- **`batch.rs`** — `BatchedLine` (carries pre-rendered output bytes plus
  a sort timestamp) and `BatchProcessor` (window-based batching with a
  min-heap on the sort key). `BatchedLine::new` parses each line *once*
  into `Printable`, extracts the timestamp, and renders to bytes — no
  double-parse.
- **`file_state.rs`** — `FileState` (per-file position + inode rotation
  tracking) and `FileStateManager`.
- **`watcher.rs`** — `MultiFileWatcher` wrapping `notify`, with
  `WatchEvent` enum and tokio channels for async event delivery.

### `src/metrics/`
- **`collector.rs`** — `ChunkMetrics` (chunks seen, bytes, duration).
- **`memory.rs`** — system memory pressure detection via `sysinfo`.

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

**Printable enum** - Memory-optimized with boxed variants. Variants are tried
most-specific to least-specific via `#[serde(untagged)]`:
- `Canonical(Box<Canonical<'a>>)` - strictly-typed HTTP log (fastest path)
- `Java(Box<Java<'a>>)` - log4j/slf4j-style Java records with stack traces
- `Message(Box<Message<'a>>)` - flexible structured log (the superset path)
- `TimeOnly(Timestamped)` - JSON we recognize *only* by timestamp
- `Json(GenericJson)` - any other JSON object
- `Logfmt(LogfmtLine)` - `key=value` lines (Heroku-style)
- `Text(String)` - plain text fallback
- Enum size optimized via boxing (Canonical/Java/Message are large)

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
- `clap_complete` - Shell completion generation for the CLI
- `serde` - Derive macros for the `Printable` log-pattern types
- `serde_json` - JSON parsing with flexible field aliases and `Cow<'a, str>` support
- `jiff` - Timestamp handling and formatting
- `owo-colors` - Terminal colors and styling
- `textwrap` - Terminal width detection
- `ansi-width` - ANSI escape sequence aware width calculation for colored text
- `tabwriter` - Elastic-tabstop column alignment
- `bytes` - Efficient byte buffer handling with `BytesMut`
- `humansize` - Binary size formatting (KB, MB, GB)
- `memory-stats` - Process RSS sampling for the memory budget
- `sysinfo` - System memory pressure detection
- `tokio` - Async runtime for multi-file coordination and event handling
- `notify` - Cross-platform file system event watching
- `glob` - Glob pattern matching for file expansion
- `thiserror` - Ergonomic error type definitions
- `miette` - Rich error diagnostics with source location tracking
- `tempfile` - Temp-file fallback for stdin `backtrack_lines` past the memory budget (also used in tests)
- `criterion` - Statistical benchmarking framework for performance testing (dev-dependency)

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
  - Comprehensive test suite (88 tests passing)
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
6. **Comprehensive Testing**: 88 tests passing with benchmark suite
7. **Rich Error Handling**: Diagnostic messages with helpful suggestions
8. **Logfmt support**: `LogfmtLine` is a first-class `Printable` variant for Heroku-style key=value lines.
9. **Numeric-or-string fields in `Message`**: HTTP fields like `status` and `size` accept either string or number JSON values via the `de_string_or_number` deserializer; previously a numeric `status` silently fell through to the GenericJson path.

### Future Enhancements (Optional)
1. **Chunk Pooling**: Vec<u8> recycling for high-throughput scenarios (5-15% improvement)
2. **Time-based Offsets**: Advanced log analysis with timestamp-based seeking
3. **Format Extensions**: Other structured log formats beyond logfmt
4. **Layout Redesign**: Improved column packing and visual layout for field display

### Maintenance Tasks
1. **Documentation**: Keep examples and guides updated with new features
2. **Performance Monitoring**: Regular benchmarking to prevent regressions  
3. **Cross-platform Testing**: Validate functionality across different systems
4. **Dependency Updates**: Keep dependencies current for security and performance

## Project memory

This project uses the trivia MCP. All memories are tagged `project:tale-ndjson`. Recall by that tag at the start of work (`recall("project:tale-ndjson")` or query with the tag filter). Add new lessons via the `session-retro` skill rather than writing one-off notes here.

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

### 2026-05-07: Mental-model alignment

A holistic review surfaced a long tail of "the docs say one thing, the code says another" drift, plus dead-code threads from the 2026-02-21 cleanup that hadn't been fully pulled. This commit fixed all of it in one pass.

**Doc/code sync**:
- README dropped the bogus "adaptive chunking strategies that automatically adjust" claim (those were deleted on 2026-02-21).
- CLAUDE.md realigned: every module path, the actual `Printable` variant set, the `lib.rs` / binary split, the `defaults.rs` / `json_profiler.rs` / `logfmt.rs` modules that were never previously documented.

**Dead code removed**:
- Methods: `MultiFileWatcher::stop`, `create_watcher_with_config`, `FileStateManager::update_position`, `files_with_new_data`, `StaticStrategy::conservative` / `with_config` / `from_config`, `ChunkedFileReader::new_with_config` / `reset`, `MemoryBudget::recommended_chunk_size` (and `MemoryPressure::chunk_size_factor`), `IoErrorExt` trait.
- Fields: `BatchedLine.parsed_json` / `_source_file` / `_line_number`, `BatchConfig._max_buffer_memory`, `_path` on `ChunkedFileReader`, `_config` on `MultiFileWatcher`, `WatcherConfig` (entire type), `_reader_id` parameter on `try_allocate`, `_config` parameter on the gone `from_config`.
- Constants: `INITIAL_CHUNK_SIZE`, `DEFAULT_MEMORY_PERCENTAGE`, `MAX_CHUNK_SIZE`, `DEFAULT_BATCH_WINDOW_MS`, `DEFAULT_LINE_CAPACITY`, `DEFAULT_OUTPUT_BUFFER_CAPACITY`, `should_chunk_by_default`.
- Misc: commented-out `tokio::time::timeout` import, three deleted dead-code tests.

**Real correctness fix — double-parse on tail path**: `BatchedLine` used to deserialize each tailed line into `serde_json::Value` solely to extract a timestamp, throw the parsed Value away, then `process_line` re-parsed the same line into `Printable` to render. Now `BatchedLine::new` parses *once* into `Printable`, extracts the timestamp through `SourcedLine::timestamp()`, and pre-renders to bytes. The `handle_tailing` consumer just writes those bytes. As a side effect, multi-file tailing now also gets `==> filename <==` headers (which it didn't before, because it bypassed `SourcedLine`).

**Real correctness fix — numeric `status`/`size` in Message**: previously `Option<Cow<'a, str>>` only deserialized JSON strings; nginx/k8s logs emit these fields as numbers and were silently falling through to the `GenericJson` path, losing the HTTP-aware rendering. Added a `de_string_or_number` visitor that accepts either string or number (allocates only on the number path) and applied it to `status`, `size`, `response_bytes`, `request_size`, `request_duration`, `upstream_time`, `upstream_header_time`, and `upstream_status`.

**DRY consolidation in `logpatterns/`** (~150 lines removed):
- Added `LayoutMetrics::current()` so each variant computes terminal-width / padding / show-time once. Was duplicated 6 times across `patterns.rs` and `logfmt.rs`.
- Added `write_cells_with_padding` helper. Was duplicated 4 times.
- Added `pad_spaces` helper using `BytesMut::resize` instead of `for _ in 0..N { extend(b" ") }` loops.
- Added `rest_to_cells` helper for walking a `#[serde(flatten)]` field.
- Added a default `fmt_pretty` method on `PrettyPrintable`. Each `Display::fmt` impl is now one line (`self.fmt_pretty(f)`) instead of seven.
- `colorize_json_value` and `colorize_map_entry` consolidated; the shared scalar logic lives in one place.

**Visibility tightening**:
- `defaults`, `errors`, `memory_budget`, `logpatterns` are now `pub(crate)` (still accessible via re-exports for `TaleError`, `MemoryBudget`, `MemoryPressure`).
- `json_profiler` is `#[cfg(debug_assertions)]` — released builds don't include it at all.
- `Args` (the clap-derived CLI struct) lives in `main.rs`. The library defines a `CliOptions` trait that `main.rs::Args` implements; the library has no clap dependency in its public surface.

**`config::*` accessor cleanup**: nine functions had `#[cfg(not(test))] return X; #[cfg(test)] return X;` — both branches identical. Collapsed.

**Mechanical**:
- `&[0x0a; 1]` → `b"\n"` in two places.
- Duplicate `tempfile = "3.21"` removed from `[dev-dependencies]` (already in `[dependencies]`).

**Net result**: cleaner module boundaries, a real perf fix on the tail path (no more double-parse), correctness fix for nginx-style numeric status codes, ~250 lines of dead code removed, docs and code finally describe the same project.
