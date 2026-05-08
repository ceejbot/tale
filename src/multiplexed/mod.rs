//! Weaving together output from more than one file.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use bytes::{Buf, BytesMut};
use miette::{IntoDiagnostic, Result};

mod batch;
mod file_state;
pub mod watcher;

use crate::config;
use crate::errors::TaleError;
use crate::logpatterns::*;

/// Handle multi-file static mode (read all files once, no following)
pub fn handle_static(paths: Vec<PathBuf>) -> Result<()> {
    use file_state::FileStateManager;

    let mut file_manager = FileStateManager::new();

    // Add all files to the manager
    for path in &paths {
        file_manager.add_file(path)?;
    }

    // Read all lines from all files (for static reading)
    let all_lines = file_manager.read_all_lines()?;

    // Collect all lines, parse and convert to SourcedLine immediately
    let mut all_sourced_lines: Vec<SourcedLine<'_>> = Vec::new();

    all_lines.iter().for_each(|(file_path, lines)| {
        lines.iter().enumerate().for_each(|(line_num, line_content)| {
            let parsed: Printable<'_> = {
                match serde_json::from_str::<Printable<'_>>(line_content) {
                    Ok(printable) => printable,
                    Err(_) => {
                        // If parsing as Printable fails, it's either invalid JSON or doesn't match any
                        // variant Use the Text fallback in either case
                        Printable::Text(line_content.to_owned())
                    }
                }
            };
            let sourced = SourcedLine::new(parsed, file_path.clone(), line_num);
            all_sourced_lines.push(sourced);
        });
    });
    // Sort using the stable multi-file sorting function
    all_sourced_lines.sort_by_key(|line| line.sort_key());

    // Output the sorted lines using existing process_line function
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);

    for wrapped in all_sourced_lines {
        wrapped.write(&mut buffer);
        outlock.write_all(buffer.chunk()).into_diagnostic()?;
        outlock.write_all(b"\n").into_diagnostic()?; // blank line between log entries
        buffer.clear();
    }

    outlock.flush().into_diagnostic()?;
    Ok(())
}

/// Handle multi-file tailing mode (watch for changes and follow)
pub async fn handle_tailing(paths: Vec<PathBuf>) -> Result<()> {
    use batch::{BatchConfig, BatchedLine, batched_with_config};
    use watcher::{WatchEvent, create_watcher};

    let mut watcher = create_watcher();
    watcher.add_files(paths).await?;

    let batch_config = BatchConfig {
        batch_window: Duration::from_millis(config::batch_window_ms()),
        max_batch_size: 1000,
    };
    let mut batch_processor = batched_with_config(batch_config);
    let (line_sender, mut batch_receiver) = batch_processor.start().await?;

    let mut watch_events = watcher.watch().await?;

    let mut outlock = io::stdout().lock();

    // Main coordination loop. Lines are pre-rendered inside `BatchedLine::new`,
    // so all this loop has to do is sort them by timestamp and ship the bytes.
    loop {
        tokio::select! {
            watch_event = watch_events.recv() => {
                match watch_event {
                    Some(WatchEvent::FileModified(path)) => {
                        if let Some(state) = watcher.file_manager_mut().get_state_mut(&path)
                            && let Ok(_changed) = state.refresh()
                            && let Ok(new_lines) = state.read_new_lines() {
                            for (line_num, line) in new_lines.into_iter().enumerate() {
                                let batched_line = BatchedLine::new(line, path.clone(), line_num);
                                if line_sender.send(batched_line).is_err() {
                                    return Err(TaleError::BatchedLineSender.into());
                                }
                            }
                        }
                    }
                    Some(WatchEvent::Error(err)) => {
                        eprintln!("Watch error: {err}");
                    }
                    Some(_) => {
                        // Other events (create, delete) — not handled yet.
                    }
                    None => break, // Watcher stopped.
                }
            }

            batch = batch_receiver.recv() => {
                match batch {
                    Some(sorted_lines) => {
                        for batched_line in sorted_lines {
                            outlock.write_all(&batched_line.rendered).into_diagnostic()?;
                        }
                        outlock.flush().into_diagnostic()?;
                    }
                    None => break, // Batch processor stopped.
                }
            }
        }
    }

    Ok(())
}
