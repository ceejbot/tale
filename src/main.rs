//! Pretty-print newline-delimited json (ndjson) logs, with a tail-compatible
//! set of options and behaviors.

use clap::Parser;
use miette::Result as MietteResult;
use tale::config::{ConfigOpts, InputMode};
#[cfg(debug_assertions)]
use tale::json_profiler;
use tale::{Args, config, multiplexed, readers};

/// Parse our options and do the thing.
#[tokio::main]
async fn main() -> MietteResult<()> {
    let args = Args::parse();

    let config = ConfigOpts::new(&args);
    config::set(config).expect("Quite improbably failed to set config OnceLock on process start.");

    let mode = config::mode();
    let result = match mode {
        InputMode::Stdin => readers::handle_stdin(),
        InputMode::SingleFile { path } => readers::handle_file(&path),
        InputMode::MultiFile { paths } => {
            if args.follow || args.sticky {
                // Multi-file tailing mode
                multiplexed::handle_tailing(paths).await
            } else {
                // Multi-file static mode (read all files once)
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

    // Convert TaleError to miette Report for display
    result.map_err(miette::Report::from)
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn offset_unit_args() {
        use tale::Args;
        use tale::config::{ConfigOpts, OffsetUnit};

        // Test bytes offset detection - test the config struct directly
        let args = Args {
            bytes: Some(100),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Bytes));
        assert_eq!(config.offset, 100);

        // Test blocks offset detection
        let args = Args {
            blocks: Some(2),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Blocks));
        assert_eq!(config.offset, 2);

        // Test lines offset detection (default)
        let args = Args {
            offset: Some(5),
            args: vec!["test.log".to_string()],
            ..Default::default()
        };
        let config = ConfigOpts::new(&args);
        assert!(matches!(config.offset_unit, OffsetUnit::Lines));
        assert_eq!(config.offset, 5);
    }

    #[test]
    fn test_cli_with_adaptation() {
        let output = std::process::Command::new("cargo")
            .args(["run", "--", "fixtures/benchmarks/medium.log"])
            .output()
            .expect("failed to execute");

        assert!(output.status.success());
        // Verify output is correct
    }
}
