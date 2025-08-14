//! Weaving together output from more than one file.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use bytes::{Buf, BytesMut};

use crate::errors::TaleError;
use crate::logpatterns::*;
use crate::{config, process_line};

/// Handle multi-file static mode (read all files once, no following)
pub fn handle_static(paths: Vec<PathBuf>) -> Result<(), TaleError> {
    use crate::file_state::FileStateManager;

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
        outlock.write_all(buffer.chunk())?;
        outlock.write_all(&[0x0a; 1])?; // blank line
        buffer.clear();
    }

    outlock.flush()?;
    Ok(())
}

/// Handle multi-file tailing mode (watch for changes and follow)
pub async fn handle_tailing(paths: Vec<PathBuf>) -> Result<(), TaleError> {
    use crate::batch::{BatchConfig, BatchedLine, create_processor_with_config};
    use crate::watcher::{WatchEvent, create_watcher};

    // Create the file watcher
    let mut watcher = create_watcher();

    // Add files to watch
    watcher.add_files(paths).await?;

    // Create batch processor with configuration from CLI
    let batch_config = BatchConfig {
        batch_window: Duration::from_millis(config::batch_window_ms()),
        max_batch_size: 1000,
        max_buffer_memory: 10 * 1024 * 1024,
    };
    let mut batch_processor = create_processor_with_config(batch_config);

    // Start the batch processor
    let (line_sender, mut batch_receiver) = batch_processor.start().await?;

    // Start watching files
    let mut watch_events = watcher.watch().await?;

    // Set up output
    let mut outlock = io::stdout().lock();
    let mut buffer = BytesMut::with_capacity(2048);

    // Main coordination loop
    loop {
        tokio::select! {
            // Handle file system events
            watch_event = watch_events.recv() => {
                match watch_event {
                    Some(WatchEvent::FileModified(path)) => {
                        // File was modified, read new lines
                        if let Some(state) = watcher.file_manager_mut().get_state_mut(&path)
                            && let Ok(_changed) = state.refresh()
                            && let Ok(new_lines) = state.read_new_lines() {
                            // Send lines to batch processor
                            for (line_num, line) in new_lines.into_iter().enumerate() {
                                let batched_line = BatchedLine::new(
                                    line,
                                    path.clone(),
                                    line_num as u64
                                );
                                match line_sender.send(batched_line) {
                                    Ok(v) => v,
                                    Err(_) => return Err(TaleError::BatchedLineSender)
                                }
                            }
                        }
                    }
                    Some(WatchEvent::Error(err)) => {
                        eprintln!("Watch error: {err}");
                    }
                    Some(_) => {
                        // Other events (create, delete) - could handle these in future
                    }
                    None => {
                        // Watcher stopped
                        break;
                    }
                }
            }

            // Handle sorted batches from batch processor
            batch = batch_receiver.recv() => {
                match batch {
                    Some(sorted_lines) => {
                        // Output the sorted batch
                        for batched_line in sorted_lines {
                            process_line(&batched_line.content, &mut buffer, &mut outlock)?;
                        }
                        outlock.flush()?;
                    }
                    None => {
                        // Batch processor stopped
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
