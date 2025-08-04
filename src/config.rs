//! A module to corral configuration to the side and into a single
//! source of truth. Provides conveniences for answering config
//! questions without having to pass batons around.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Hold our configuration.
pub static CONFIG: OnceLock<ConfigOpts> = OnceLock::new();

pub fn config() -> &'static ConfigOpts {
    CONFIG
        .get()
        .expect("programmer error: tried to access configuration before it was set")
}

pub fn set(input: ConfigOpts) -> Result<(), ConfigOpts> {
    CONFIG.set(input)
}

// The public interface for config

pub fn tailing() -> bool {
    config().tailing
}

pub fn sticky() -> bool {
    config().sticky
}

pub fn offset() -> i64 {
    config().offset
}

pub fn offset_unit() -> Offset {
    config().offset_unit
}

pub fn show_time() -> bool {
    config().show_time
}

pub fn batch_window_ms() -> u64 {
    config().batch_window_ms
}

pub fn mode() -> &'static InputMode {
    &config().mode
}

/// A sensible holder for our configuration.
#[derive(Debug, Clone, Default)]
pub struct ConfigOpts {
    pub tailing: bool,
    pub sticky: bool,
    pub offset: i64,
    pub offset_unit: Offset,
    pub show_time: bool,
    pub batch_window_ms: u64,
    pub mode: InputMode,
}

#[derive(Debug, Clone, Default, Copy)]
pub enum Offset {
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

fn is_glob(maybe: &str) -> bool {
    maybe.contains('?') || maybe.contains('*') || maybe.contains('[')
}

/// Amongst our list of files to tail we might have a glob pattern
/// to expand. If so, we find matches. Otherwise, we add that path
/// to our list directly.
fn expand_globs(args: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut all_paths = Vec::new();

    for candidate in args {
        if is_glob(candidate.as_str()) {
            let pattern = glob::glob(&candidate)?;
            for entry in pattern {
                if let Ok(fpath) = entry {
                    if fpath.is_file() {
                        all_paths.push(fpath);
                    }
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

fn handle_possible_paths(args: &[String]) -> Vec<PathBuf> {
    if let Ok(paths) = expand_globs(args) {
        return paths;
    }

    todo!()
}

impl ConfigOpts {
    pub fn new(args: &crate::Args) -> Self {
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
                    let paths = handle_possible_paths(vec![only.clone()].as_slice());
                    if paths.len() == 1 {
                        (
                            InputMode::SingleFile {
                                path: PathBuf::from(only),
                            },
                            None,
                        )
                    } else {
                        (InputMode::MultiFile { paths }, None)
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
                    let paths = handle_possible_paths(args.args.as_slice());
                    (InputMode::MultiFile { paths }, None)
                }
            }
            _ => {
                // More than two paths and/or globs.
                // We still want to know if the first arg is an offset.
                let paths = handle_possible_paths(args.args.as_slice());
                (InputMode::MultiFile { paths }, None)
            }
        };

        let (offset, offset_unit) = if let Some(blocks) = args.blocks {
            (blocks, Offset::Blocks)
        } else if let Some(bytes) = args.bytes {
            (bytes, Offset::Bytes)
        } else if let Some(lines) = args.offset {
            (lines, Offset::Lines)
        } else if let Some(offset) = maybe_offset {
            (offset, Offset::Lines)
        } else {
            (0, Offset::Lines)
        };

        Self {
            tailing: args.follow || args.sticky,
            sticky: args.sticky,
            offset,
            offset_unit,
            show_time: args.timestamps,
            batch_window_ms: args.window,
            mode,
        }
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
            args: vec!["-4".to_string()],
        };
        let config = ConfigOpts::new(&args);
        assert_eq!(config.offset, -5);
        assert!(matches!(config.mode, InputMode::Stdin));
    }

    #[test]
    fn glob_expansions() {
        let fixture_glob = "./fixtures/*.log".to_string();
        let results = expand_globs(&[fixture_glob]).expect("this list of paths should expand successfully");
        assert_eq!(results.len(), 7); // changes if we add fixtures to that directory
        assert_eq!(
            results.as_slice(),
            vec![
                PathBuf::from("fixtures/ascii_colors.log"),
                PathBuf::from("fixtures/garbage_prefix.log"),
                PathBuf::from("fixtures/java_stacktrace.log"),
                PathBuf::from("fixtures/just_loglines.log"),
                PathBuf::from("fixtures/mixed_json_types.log"),
                PathBuf::from("fixtures/mixed_text_json.log"),
                PathBuf::from("fixtures/windows_line_endings.log")
            ]
        );
    }
}
