//! Pretty-print newline-delimited json (ndjson) logs, with a tail-compatible
//! set of options and behaviors.

use clap::Parser;
use clap_complete::generate;
use miette::Result as MietteResult;
use tale_ndjson::config::{ConfigOpts, InputMode};
#[cfg(debug_assertions)]
use tale_ndjson::json_profiler;
use tale_ndjson::{Args, config, multiplexed, readers};

/// Parse our options and do the thing.
#[tokio::main]
async fn main() -> MietteResult<()> {
    // the args struct is defined in lib.rs for various reasons
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

    result
}
