//! A module to corral configuration to the side and into a single
//! source of truth. Provides conveniences for answering config
//! questions without having to pass batons around.

use std::path::PathBuf;

/// A sensible holder for our configuration.
#[derive(Debug, Clone, Default)]
pub struct ConfigOpts {
    pub tailing: bool,
    pub sticky: bool,
    pub offset: i64,
    pub offset_unit: OffsetUnit,
    pub show_time: bool,
    pub batch_window_ms: u64,
    pub mode: InputMode,
    pub force_chunked: bool,
    pub disable_chunked: bool,
    pub no_file_names: bool,
    pub all_file_names: bool,
    pub max_memory: Option<usize>,
    #[cfg(debug_assertions)]
    pub profile_json: bool,
}

#[derive(Debug, Clone, Default, Copy)]
pub enum OffsetUnit {
    #[default]
    Lines,
    Blocks,
    Bytes,
}

/// Operation modes for tale
#[derive(Debug, Clone, Default)]
pub enum InputMode {
    /// Read from stdin
    #[default]
    Stdin,
    /// Read from a single file
    SingleFile { path: PathBuf },
    /// Watch multiple files
    MultiFile { paths: Vec<PathBuf> },
}

// Production implementation: Simple OnceLock for fast access
#[cfg(not(test))]
mod runtime {
    use std::sync::OnceLock;

    use super::ConfigOpts;

    /// Hold our configuration - production uses simple OnceLock
    pub static CONFIG: OnceLock<ConfigOpts> = OnceLock::new();

    /// Get reference to configuration
    pub fn config() -> &'static ConfigOpts {
        CONFIG
            .get()
            .expect("programmer error: tried to access configuration before it was set")
    }

    /// Set configuration - one-time initialization only
    pub fn set(input: ConfigOpts) -> Result<(), Box<ConfigOpts>> {
        match CONFIG.set(input) {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(e)),
        }
    }
}

// Test implementation: Thread-local storage for isolation
#[cfg(test)]
mod runtime {
    use std::cell::RefCell;

    use super::ConfigOpts;

    // Test implementation uses thread-local storage for isolation
    thread_local! {
        pub static TEST_CONFIG: RefCell<Option<ConfigOpts>> = const { RefCell::new(None) };
    }

    /// Get configuration - returns owned value from thread-local storage
    pub fn config() -> ConfigOpts {
        TEST_CONFIG.with(|cfg| cfg.borrow().as_ref().cloned().unwrap_or_else(ConfigOpts::default))
    }

    /// Set configuration - can be called multiple times per thread
    pub fn set(input: ConfigOpts) -> Result<(), Box<ConfigOpts>> {
        TEST_CONFIG.with(|cfg| {
            *cfg.borrow_mut() = Some(input);
        });
        Ok(())
    }

    /// Test-only helper to modify config in place
    pub fn update<F>(f: F)
    where
        F: FnOnce(&mut ConfigOpts),
    {
        TEST_CONFIG.with(|cfg| {
            let mut borrowed = cfg.borrow_mut();
            if borrowed.is_none() {
                *borrowed = Some(ConfigOpts::default());
            }
            if let Some(ref mut config) = borrowed.as_mut() {
                f(config);
            }
        });
    }

    /// Test-only helper for temporary config changes
    pub fn with_config<F, R>(new_config: ConfigOpts, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // Save old config
        let old_config = TEST_CONFIG.with(|cfg| cfg.borrow().clone());

        // Set new config
        TEST_CONFIG.with(|cfg| {
            *cfg.borrow_mut() = Some(new_config);
        });

        // Run the test
        let result = f();

        // Restore old config
        TEST_CONFIG.with(|cfg| {
            *cfg.borrow_mut() = old_config;
        });

        result
    }
}

// Re-export the runtime implementation as the public API
use miette::Result;
pub use runtime::{config, set};
#[cfg(test)]
pub use runtime::{update, with_config};

use crate::defaults::{SystemDefaults, get_system_config};
use crate::errors::TaleError;

// Public convenience accessors - these work with both implementations
pub fn tailing() -> bool {
    #[cfg(not(test))]
    return config().tailing;
    #[cfg(test)]
    return config().tailing;
}

pub fn sticky() -> bool {
    #[cfg(not(test))]
    return config().sticky;
    #[cfg(test)]
    return config().sticky;
}

pub fn offset() -> i64 {
    #[cfg(not(test))]
    return config().offset;
    #[cfg(test)]
    return config().offset;
}

pub fn offset_unit() -> OffsetUnit {
    #[cfg(not(test))]
    return config().offset_unit;
    #[cfg(test)]
    return config().offset_unit;
}

pub fn show_time() -> bool {
    #[cfg(not(test))]
    return config().show_time;
    #[cfg(test)]
    return config().show_time;
}

pub fn batch_window_ms() -> u64 {
    #[cfg(not(test))]
    return config().batch_window_ms;
    #[cfg(test)]
    return config().batch_window_ms;
}

pub fn force_chunked() -> bool {
    #[cfg(not(test))]
    return config().force_chunked;
    #[cfg(test)]
    return config().force_chunked;
}

pub fn disable_chunked() -> bool {
    #[cfg(not(test))]
    return config().disable_chunked;
    #[cfg(test)]
    return config().disable_chunked;
}

pub fn mode() -> InputMode {
    #[cfg(not(test))]
    return config().mode.clone();
    #[cfg(test)]
    return config().mode;
}

/// Unescape shell-escaped glob patterns (e.g., \* -> *, \? -> ?, \[ -> [)
fn unescape_glob_pattern(pattern: &str) -> String {
    let mut result = String::new();
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Check if next character is a glob metacharacter
            if let Some(&next_ch) = chars.peek() {
                if matches!(next_ch, '*' | '?' | '[' | ']' | '{' | '}') {
                    // Skip the backslash and add the escaped character
                    chars.next();
                    result.push(next_ch);
                } else {
                    // Not escaping a glob metacharacter, keep the backslash
                    result.push(ch);
                }
            } else {
                // Backslash at end of string
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Check if a string contains glob patterns, including shell-escaped ones
fn is_glob(maybe: &str) -> bool {
    // Check for unescaped glob patterns, for users who turn on noglob.
    if maybe.contains('?') || maybe.contains('*') || maybe.contains('[') || maybe.contains('{') {
        return true;
    }

    // Check for shell-escaped glob patterns
    maybe.contains("\\*") || maybe.contains("\\?") || maybe.contains("\\[") || maybe.contains("\\{")
}

/// Amongst our list of files to tail we might have a glob pattern
/// to expand. If so, we find matches. Otherwise, we add that path
/// to our list directly.
fn expand_globs(args: &[String]) -> Result<Vec<PathBuf>, TaleError> {
    let mut all_paths = Vec::new();

    for candidate in args {
        if is_glob(candidate.as_str()) {
            // Unescape shell-escaped glob patterns before expansion
            let unescaped_pattern = unescape_glob_pattern(candidate);
            let pattern = glob::glob(&unescaped_pattern)?;
            for fpath in pattern.flatten() {
                if fpath.is_file() {
                    all_paths.push(fpath);
                }
            }
        } else {
            let fpath = PathBuf::from(candidate);
            if fpath.exists() && fpath.is_file() {
                all_paths.push(fpath);
            }
        }
    }
    all_paths.sort();
    Ok(all_paths)
}

fn handle_possible_paths(args: &[String]) -> Result<Vec<PathBuf>, TaleError> {
    match expand_globs(args) {
        Ok(paths) => {
            if paths.is_empty() {
                // No files matched the glob pattern(s)
                let patterns: Vec<String> = args.iter().map(|s| format!("'{}'", s)).collect();
                Err(TaleError::from(Box::new(crate::errors::FileError::NotFound {
                    path: PathBuf::from(patterns.join(", ")),
                    similar_files: vec![
                        "Check if the glob pattern is correct".to_string(),
                        "Verify the files exist in the specified directory".to_string(),
                        "Try using an absolute path".to_string(),
                    ],
                })))
            } else {
                Ok(paths)
            }
        }
        Err(e) => {
            // Glob expansion failed - could be invalid pattern or I/O error
            Err(e)
        }
    }
}

impl ConfigOpts {
    pub fn new(args: &crate::Args) -> Result<Self> {
        // Get production defaults
        let system_config = get_system_config();
        let (mode, maybe_offset) = match args.args.len() {
            0 => (InputMode::Stdin, None),
            1 => {
                let only = &args.args[0];
                if (only.starts_with('-') || only.starts_with('+'))
                    && only.len() > 1
                    && let Ok(offset) = only.parse::<i64>()
                {
                    // It's a numeric offset like "-4" or "+4"
                    (InputMode::Stdin, Some(offset))
                } else {
                    // It's a filename or a glob
                    if is_glob(only) {
                        // It's a glob pattern, handle as multi-file
                        let paths = handle_possible_paths(vec![only.clone()].as_slice())?;
                        (InputMode::MultiFile { paths }, None)
                    } else {
                        // It's a single filename (may or may not exist) - always treat as SingleFile
                        (
                            InputMode::SingleFile {
                                path: PathBuf::from(only),
                            },
                            None,
                        )
                    }
                }
            }
            2 => {
                let (first, second) = (&args.args[0], &args.args[1]);

                // Check if first arg is an offset
                if let Ok(offset) = first.parse::<i64>() {
                    // offset + single file
                    (
                        InputMode::SingleFile {
                            path: PathBuf::from(second),
                        },
                        Some(offset),
                    )
                } else {
                    // Two file paths or globs: we're multifile for sure.
                    let paths = handle_possible_paths(args.args.as_slice())?;
                    (InputMode::MultiFile { paths }, None)
                }
            }
            _ => {
                // More than two paths and/or globs.
                // We still want to know if the first arg is an offset.
                let paths = handle_possible_paths(args.args.as_slice())?;
                (InputMode::MultiFile { paths }, None)
            }
        };

        let (offset, offset_unit) = if let Some(blocks) = args.blocks {
            (blocks, OffsetUnit::Blocks)
        } else if let Some(bytes) = args.bytes {
            (bytes, OffsetUnit::Bytes)
        } else if let Some(lines) = args.offset {
            (lines, OffsetUnit::Lines)
        } else if let Some(offset) = maybe_offset {
            (offset, OffsetUnit::Lines)
        } else {
            (0, OffsetUnit::Lines)
        };

        // Apply production defaults
        let max_memory = args.max_memory.unwrap_or_else(|| {
            // Use production default memory budget
            let system_percentage = system_config.memory_percentage;
            if let Some(memory_stats) = memory_stats::memory_stats() {
                let system_memory = memory_stats.physical_mem;
                let calculated = (system_memory as f64 * system_percentage / 100.0) as usize;
                calculated.clamp(SystemDefaults::MIN_MEMORY_BUDGET, SystemDefaults::MAX_MEMORY_BUDGET)
            } else {
                // Fallback to reasonable default
                system_config.max_memory_mb * 1024 * 1024
            }
        });

        // Determine chunking behavior based on production defaults if not specified
        let force_chunked = if args.chunked {
            true
        } else if args.no_chunked {
            false
        } else {
            // Use production default based on preset
            system_config.force_chunked
        };

        Ok(Self {
            tailing: args.follow || args.sticky,
            sticky: args.sticky,
            offset,
            offset_unit,
            show_time: args.timestamps,
            batch_window_ms: args.window,
            mode,
            force_chunked,
            disable_chunked: args.no_chunked,
            no_file_names: args.quiet,
            all_file_names: args.verbose,
            max_memory: Some(max_memory),
            #[cfg(debug_assertions)]
            profile_json: args.profile_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complicated_args() {
        let args = crate::Args {
            timestamps: true,
            follow: true,
            sticky: false,
            blocks: None,
            bytes: Some(-5),
            offset: None,
            verbose: false,
            quiet: false,
            window: 250,
            chunked: false,
            no_chunked: false,
            args: vec!["-4".to_string()],
            max_memory: Some(10_000_000_000),
            completions: None,
            #[cfg(debug_assertions)]
            profile_json: false,
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert_eq!(config.offset, -5);
        assert!(matches!(config.mode, InputMode::Stdin));
    }

    #[test]
    fn glob_expansions() {
        let fixture_glob = "./fixtures/*.log".to_string();
        let results = expand_globs(&[fixture_glob]).expect("this list of paths should expand successfully");
        assert_eq!(results.len(), 8); // changes if we add fixtures to that directory
        assert_eq!(
            results.as_slice(),
            vec![
                PathBuf::from("fixtures/ascii_colors.log"),
                PathBuf::from("fixtures/garbage_prefix.log"),
                PathBuf::from("fixtures/java_stacktrace.log"),
                PathBuf::from("fixtures/just_loglines.log"),
                PathBuf::from("fixtures/log4j.log"),
                PathBuf::from("fixtures/mixed_json_types.log"),
                PathBuf::from("fixtures/mixed_text_json.log"),
                PathBuf::from("fixtures/windows_line_endings.log")
            ]
        );
    }

    #[test]
    fn can_unescape_glob_pattern() {
        // Test basic unescaping
        assert_eq!(unescape_glob_pattern("\\*.log"), "*.log");
        assert_eq!(unescape_glob_pattern("test\\?.txt"), "test?.txt");
        assert_eq!(unescape_glob_pattern("\\[abc\\]"), "[abc]");

        // Test mixed patterns
        assert_eq!(unescape_glob_pattern("\\*.log\\?"), "*.log?");
        assert_eq!(unescape_glob_pattern("test\\*file\\?.log"), "test*file?.log");

        // Test non-glob backslashes (should be preserved)
        assert_eq!(unescape_glob_pattern("file\\name.txt"), "file\\name.txt");
        assert_eq!(unescape_glob_pattern("path\\to\\file"), "path\\to\\file");

        // Test already unescaped patterns (should be unchanged)
        assert_eq!(unescape_glob_pattern("*.log"), "*.log");
        assert_eq!(unescape_glob_pattern("test?.txt"), "test?.txt");

        // Test empty and edge cases
        assert_eq!(unescape_glob_pattern(""), "");
        assert_eq!(unescape_glob_pattern("\\"), "\\");
        assert_eq!(unescape_glob_pattern("file\\"), "file\\");
    }

    #[test]
    fn is_glob_with_escaped_patterns_works() {
        // Test escaped glob patterns
        assert!(is_glob("\\*.log"));
        assert!(is_glob("test\\?.txt"));
        assert!(is_glob("\\[abc]"));

        // Test unescaped glob patterns (existing functionality)
        assert!(is_glob("*.log"));
        assert!(is_glob("test?.txt"));
        assert!(is_glob("[abc]"));

        // Test non-glob patterns
        assert!(!is_glob("file.log"));
        assert!(!is_glob("test.txt"));
        assert!(!is_glob("path/to/file"));

        // Test backslashes that don't escape glob chars
        assert!(!is_glob("file\\name.txt"));
        assert!(!is_glob("path\\to\\file"));
    }

    #[test]
    fn can_expand_escaped_globs() {
        // This test requires the fixtures directory to exist
        // Test that escaped glob patterns work the same as unescaped ones
        let escaped_fixture_glob = ".\\*/fixtures/\\*.log".to_string();
        let normal_fixture_glob = "./fixtures/*.log".to_string();

        // Both should expand to the same files (if fixtures exist)
        if let (Ok(escaped_results), Ok(normal_results)) = (
            expand_globs(&[escaped_fixture_glob]),
            expand_globs(&[normal_fixture_glob]),
        ) {
            assert_eq!(escaped_results, normal_results);
        }
    }

    #[test]
    fn can_modify_config() {
        let initial_config = ConfigOpts {
            tailing: false,
            sticky: false,
            offset: 10,
            offset_unit: OffsetUnit::Lines,
            show_time: false,
            batch_window_ms: 250,
            mode: InputMode::Stdin,
            force_chunked: false,
            disable_chunked: false,
            ..Default::default()
        };

        // Use with_config to isolate this test
        with_config(initial_config.clone(), || {
            // Test the update function
            update(|cfg| {
                cfg.tailing = true;
                cfg.offset = 20;
                cfg.show_time = true;
            });

            // Verify the changes
            assert!(tailing());
            assert_eq!(offset(), 20);
            assert!(show_time());
        });
    }

    #[test]
    fn can_test_with_config() {
        let original_config = ConfigOpts::default();
        set(original_config.clone()).expect("should set config");

        let original_offset = offset();
        let original_tailing = tailing();

        // Test with temporary config
        let result = with_config(
            ConfigOpts {
                tailing: true,
                sticky: false,
                offset: 42,
                offset_unit: OffsetUnit::Bytes,
                show_time: true,
                batch_window_ms: 500,
                mode: InputMode::Stdin,
                force_chunked: true,
                disable_chunked: false,
                ..Default::default()
            },
            || {
                // Inside this closure, config should be changed
                assert_eq!(offset(), 42);
                assert!(tailing());
                assert_eq!(batch_window_ms(), 500);
                assert!(force_chunked());

                // Return a value to verify the closure ran
                "test_successful"
            },
        );

        // After the closure, config should be restored
        assert_eq!(offset(), original_offset);
        assert_eq!(tailing(), original_tailing);
        assert_eq!(result, "test_successful");
    }

    #[test]
    fn concurrent_access_to_test_config() {
        use std::thread;
        use std::time::Duration;

        // Set different configs in different threads to verify isolation
        let handles: Vec<_> = (0..3)
            .map(|i| {
                thread::spawn(move || {
                    let config = ConfigOpts {
                        offset: i * 10,
                        tailing: i % 2 == 0,
                        show_time: i % 2 == 1,
                        ..ConfigOpts::default()
                    };

                    set(config).expect("should set config");

                    // Sleep a bit to let other threads potentially interfere
                    thread::sleep(Duration::from_millis(10));

                    // Verify our config is still correct
                    assert_eq!(offset(), i * 10);
                    assert_eq!(tailing(), i % 2 == 0);
                    assert_eq!(show_time(), i % 2 == 1);

                    i
                })
            })
            .collect();

        // Collect results
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("test results should always be ok"))
            .collect();
        assert_eq!(results, vec![0, 1, 2]);
    }

    #[test]
    fn config_accessors_work() {
        let test_config = ConfigOpts {
            tailing: true,
            sticky: true,
            offset: -100,
            offset_unit: OffsetUnit::Blocks,
            show_time: true,
            batch_window_ms: 1000,
            mode: InputMode::SingleFile {
                path: PathBuf::from("test.log"),
            },
            force_chunked: true,
            disable_chunked: false,
            ..Default::default()
        };

        // Use with_config to isolate this test
        with_config(test_config, || {
            // Test all accessor functions
            assert!(tailing());
            assert!(sticky());
            assert_eq!(offset(), -100);
            assert!(matches!(offset_unit(), OffsetUnit::Blocks));
            assert!(show_time());
            assert_eq!(batch_window_ms(), 1000);
            assert!(matches!(mode(), InputMode::SingleFile { .. }));
            assert!(force_chunked());
            assert!(!disable_chunked());
        });
    }
}
