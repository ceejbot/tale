//! Pretty-print newline-delimited json (ndjson) logs, with a tail-compatible
//! set of options and behaviors.

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use clap_complete::generate;
use miette::Result as MietteResult;
use tale_ndjson::config::{CliOptions, ConfigOpts, InputMode};
#[cfg(debug_assertions)]
use tale_ndjson::json_profiler;
use tale_ndjson::{config, multiplexed, readers};

/// CLI argument struct. Lives in the binary crate so the library doesn't pull
/// clap into its public surface; see `tale_ndjson::config::CliOptions` for the
/// trait used by the lib to read these values.
#[derive(Debug, Clone, Parser, Default)]
#[clap(name="tale", version, styles = v3_styles(), max_term_width = 100)]
/// A tail-compatible tool for pretty-printing ndjson files, especially logs.
///
/// Tale displays the colorfully-formatted contents of FILE, by default stdin,
/// to stdout. It highlights the fields likely to appear in log lines for
/// servers, such as level or severity, the log message, timestamps, and so
/// on. It also displays every field that shows up in the log line, using
/// the color theme you have set in your terminal.
///
/// Lines that are invalid json are printed intact, without formatting.
///
/// Tale can also follow and display more than one file at a time, with
/// header decoration options like `tail`'s.
pub struct Args {
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long)]
    pub follow: bool,
    /// Follow the file, also checking to see if has been renamed or has an new
    /// inode number. If the file does not exist yet, wait and display it
    /// from the beginning if and when it is created.
    #[arg(short = 'F', long)]
    pub sticky: bool,
    /// Accepted for `tail` compatibility but ignored (use -c for byte offsets).
    #[arg(short, long, group = "units")]
    pub blocks: Option<i64>,
    /// Start tailing the input offset by ±N bytes; e.g., to skip garbage.
    #[arg(short = 'c', long, group = "units")]
    pub bytes: Option<i64>,
    /// Start tailing the input offset by ±N lines.
    #[arg(short = 'n', long, group = "units")]
    pub offset: Option<i64>,
    /// When following more than one file, show a header with the file name
    /// along with every line from that file.
    #[arg(short, long)]
    pub verbose: bool,
    /// Do not ever show file name headers when following more than one file.
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    // these options are unique to `tale`
    /// Show timestamps, which are hidden by default.
    #[arg(short, long)]
    pub timestamps: bool,
    /// Batch window size for multi-file tailing (in milliseconds).
    #[arg(long, default_value = "250", hide = true)]
    pub window: u64,
    /// Generate completions for the given shell.
    #[arg(long, hide = true)]
    pub completions: Option<clap_complete::Shell>,

    // Performance and memory tuning options
    /// Force use of chunked file processing for better memory efficiency on
    /// large files.
    #[arg(long, hide = true)]
    pub chunked: bool,
    /// Disable chunked file processing and always use streaming (might use more
    /// memory).
    #[arg(long, conflicts_with = "chunked", hide = true)]
    pub no_chunked: bool,
    /// Set a limit on how much memory can be used in file buffers
    #[arg(short, long, hide = true)]
    pub max_memory: Option<usize>,
    /// Print JSON parsing profile report after processing (debug builds only)
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub profile_json: bool,

    /// (offset) [file ...] where offset can be +N, -N, or N.
    #[arg(allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl CliOptions for Args {
    fn follow(&self) -> bool {
        self.follow
    }
    fn sticky(&self) -> bool {
        self.sticky
    }
    fn bytes(&self) -> Option<i64> {
        self.bytes
    }
    fn offset(&self) -> Option<i64> {
        self.offset
    }
    fn verbose(&self) -> bool {
        self.verbose
    }
    fn quiet(&self) -> bool {
        self.quiet
    }
    fn timestamps(&self) -> bool {
        self.timestamps
    }
    fn window(&self) -> u64 {
        self.window
    }
    fn chunked(&self) -> bool {
        self.chunked
    }
    fn no_chunked(&self) -> bool {
        self.no_chunked
    }
    fn max_memory(&self) -> Option<usize> {
        self.max_memory
    }
    fn args(&self) -> &[String] {
        &self.args
    }
    #[cfg(debug_assertions)]
    fn profile_json(&self) -> bool {
        self.profile_json
    }
}

/// I like my clap help styled the old way.
fn v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// Parse our options and do the thing.
#[tokio::main]
async fn main() -> MietteResult<()> {
    let args = Args::parse();

    if let Some(shell) = args.completions {
        use clap::CommandFactory;
        let mut app = Args::command();
        generate(shell, &mut app, "tale", &mut std::io::stdout());
        return Ok(());
    }

    let config = ConfigOpts::new(&args).unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    });
    config::set(config).expect("Quite improbably failed to set config OnceLock on process start.");

    let mode = config::mode();
    let result = match mode {
        InputMode::Stdin => readers::handle_stdin(),
        InputMode::SingleFile { path } => readers::handle_file(&path).await,
        InputMode::MultiFile { paths } => {
            if args.follow || args.sticky {
                multiplexed::handle_tailing(paths).await
            } else {
                multiplexed::handle_static(paths)
            }
        }
    };

    // Print JSON profiling report automatically in debug builds
    #[cfg(debug_assertions)]
    {
        eprintln!();
        json_profiler::print_report();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn offset_unit_args() {
        use tale_ndjson::config::OffsetUnit;

        // Bytes offset detection.
        let args = Args {
            bytes: Some(100),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert!(matches!(config.offset_unit, OffsetUnit::Bytes));
        assert_eq!(config.offset, 100);

        // Lines offset detection (default).
        let args = Args {
            offset: Some(5),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args).expect("Config should be valid for test");
        assert!(matches!(config.offset_unit, OffsetUnit::Lines));
        assert_eq!(config.offset, 5);
    }

    #[test]
    fn can_run_cli_and_emit_help() {
        let output = std::process::Command::new("cargo")
            .args(["run", "--", "--help"])
            .output()
            .expect("failed to execute");

        assert!(output.status.success());
    }
}
