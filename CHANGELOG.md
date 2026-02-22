# Changelog

All notable changes to this project are	documented in this file.
We use [semantic versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-02-22

### 🚀 Features

- Feat: add --completions <shell> to generate completions

The usual! Clap is so handy.
- Feat: Finish implementing the conservative strategy

and take care of an unfinished implementation task in
BackSeekingProcessor.
- Feat: elastic tabstop column alignment via tabwriter

Replaced the sucky greedy left-to-right column packer with a
less-bad column alignment using tabwriter. Unified colorizing
types via colorize_map_entry() and tweaked colors for pop.
Switched to vertical pipe as the separation character between
the log level display and the key/value pairs section.

Nuked dead code.

### 📚 Documentation

- Doc: Internal docs cleanup.

jokes are all gone :(
- Docs: Finally, update the readme.

### 🔧 Misc

- Chore: finish the mock memory implementation

For memory pressure testing improvements.
- Chore: Address all the TODO items

No big changes, but error message improvements mostly, and some
notes about possible implementation improvements.
- Chore: benchmarks and documentation

The benchmarks are now in the manifest for use with `cargo bench`.
Project documentation and the readme are now closer to up to date.
- Chore: updated deps, cleaned up clippy

Time marches ever onward, and things change.
- Chore: rip out unused stuff; remove useless complexity

- removed `is_memory_constrained()` dead function
- removed per-reader tracking in `MemoryBudget`; removed the
  system memory polling in the inner state
- ripped out a bunch of stuff we don't care about from
  `ChunkMetrics`-- we now track only count/bytes/duration
  for chunk reads
- updated the tests and benchmarks

Most of the above turned out not to matter.
- Chore: more dead code removal

Trying to clean up unused options.

`--blocks` is now accepted but ignored so we're compatible
with tail. This option is useless with this tool, however.
`--bytes` is still useful because you might want to skip some
garbage at the start of a file. Shrugmoji.
- INFRACTION

## [0.2.1] - 2025-08-25

### 🐛 Bug Fixes

- Fix: Worked around a bug in git-cliff with removing headers

### 🔧 Misc

- Made minor release tooling tweaks to the justfile
- Changed crate name to `tale-ndjson` to avoid collision; bin is still `tale`
- Wrote some library use examples
- Integrated `git-cliff` for changelog generation

## [0.2.0] - 2025-08-17

### Added
- **Multi-file tailing support**: Watch and display multiple files simultaneously
- **Timestamp-based line batching**: Chronological ordering of multi-file output
- **Configurable batch windows**: Control timing for multi-file coordination (default: 250ms)
- **Glob pattern support**: Use wildcards to specify multiple files (e.g., `*.log`)
- **File rotation detection**: Inode-based detection with sticky follow mode (`-F`)
- **Cross-platform file watching**: Async file system event monitoring
- **Rich error diagnostics**: Comprehensive error handling with `thiserror` + `miette`
- **File similarity suggestions**: Edit distance algorithm for typo detection
- **Memory budget management**: System-wide memory tracking and pressure detection
- **Chunked file processing architecture**: Memory-efficient large file handling
- **Strategy pattern for chunk sizing**: Adaptive, conservative, and static strategies
- **Performance metrics collection**: Detailed timing and throughput tracking
- **Complete stdin offset support**: All `tail`-compatible offset modes (`-n`, `-c`, `-b`)

### Changed
- **Major architecture refactor**: Separated readers subsystem with processor selection
- **Zero-copy JSON parsing**: `Cow<'a, str>` fields reduce allocations by ~1.8M/sec
- **Custom column layout engine**: Replaced `term_grid` with direct buffer writing
- **Direct buffer formatting**: Pre-compiled ANSI sequences eliminate `format!` calls
- **Memory layout optimization**: Boxed enum variants reduce `Printable` size 360 → 40 bytes
- **StdinProcessor consolidation**: Unified stdin handling eliminates ~80 lines duplication
- **BackSeekingProcessor**: Renamed from `SimpleFileProcessor` for clarity
- **Enhanced test coverage**: 97 unit tests with comprehensive edge case handling

### Performance Improvements
- **28-37% overall speedup**: 616ms → 443ms on 23MB files
- **Canonical log type**: 25-34% improvement for well-structured HTTP logs
- **Memory efficiency**: 1.8% memory footprint (4.2MB for 235MB files)
- **387K lines/sec throughput**: Sustained performance on large files
- **CPU-optimized**: 74% CPU, 26% I/O - JSON parsing is the bottleneck

### Fixed
- **Chunked skip_lines**: Proper partial chunk consumption and state management
- **Boundary handling**: Correct line parsing across chunk boundaries
- **Negative offset support**: Memory-bounded circular buffers for bytes/blocks
- **File processor selection**: Clear capability-based routing
- **Error message clarity**: Color-coded diagnostics with source location tracking

## [0.1.0] - 2025-07-30

### Added
- Basic NDJSON log pretty-printing
- Single file processing
- Basic tail-compatible offset support
- Color-coded output with terminal width detection
- JSON parsing with flexible field aliases
- Initial performance optimization

[0.2.0]: https://github.com/ceejbot/tale/compare/v0.1.0...v0.2.0
