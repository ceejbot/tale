//! Multi-file watching using notify and async-watcher.
//!
//! This module handles file system event monitoring for multiple files,
//! coordinating with the file state manager and batch processor.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::file_state::FileStateManager;

/// Events that can occur during file watching
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A file was modified (new content available)
    FileModified(PathBuf),
    /// A file was created
    FileCreated(PathBuf),
    /// A file was deleted
    FileDeleted(PathBuf),
    /// A file was moved/renamed
    FileMoved { from: PathBuf, to: PathBuf },
    /// An error occurred while watching
    Error(String),
}

/// Configuration for the file watcher
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// How long to wait between file system checks
    pub poll_interval: Duration,
    /// Maximum number of events to buffer
    pub event_buffer_size: usize,
    /// Whether to use polling fallback for unreliable filesystems
    pub use_polling: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
            event_buffer_size: 1000,
            use_polling: false,
        }
    }
}

/// Multi-file watcher using notify and async coordination
pub struct MultiFileWatcher {
    /// Configuration for this watcher
    _config: WatcherConfig,
    /// File state manager
    file_manager: FileStateManager,
    /// Channel for sending watch events
    event_sender: Option<mpsc::UnboundedSender<WatchEvent>>,
    /// The notify watcher instance
    _watcher: Option<RecommendedWatcher>,
    /// Handle to the event processing task
    _task_handle: Option<JoinHandle<()>>,
}

impl MultiFileWatcher {
    /// Create a new MultiFileWatcher
    pub fn new(_config: WatcherConfig) -> Self {
        Self {
            _config,
            file_manager: FileStateManager::new(),
            event_sender: None,
            _watcher: None,
            _task_handle: None,
        }
    }

    /// Add files to be watched
    pub async fn add_files<I, P>(&mut self, paths: I) -> Result<()>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for path in paths {
            self.file_manager.add_file_for_tailing(path)?;
        }

        Ok(())
    }

    /// Start watching files and return a stream of events
    pub async fn watch(&mut self) -> Result<mpsc::UnboundedReceiver<WatchEvent>> {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        // Create a channel for notify events
        let (notify_sender, notify_receiver) = std::sync::mpsc::channel();

        // Create the notify watcher
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            if let Err(e) = notify_sender.send(result) {
                eprintln!("Failed to send notify event: {e}");
            }
        })?;

        // Watch all tracked files
        for file_path in self.file_manager.tracked_files() {
            watcher
                .watch(file_path, RecursiveMode::NonRecursive)
                .with_context(|| format!("Failed to watch file: {}", file_path.display()))?;
        }

        // Spawn a task to process notify events and convert them to WatchEvents
        let event_sender_clone = event_sender.clone();
        let task_handle = tokio::spawn(async move {
            while let Ok(result) = notify_receiver.recv() {
                match result {
                    Ok(event) => {
                        if let Some(watch_event) = Self::convert_notify_event(event) {
                            if event_sender_clone.send(watch_event).is_err() {
                                break; // Receiver dropped
                            }
                        }
                    }
                    Err(e) => {
                        let error_event = WatchEvent::Error(format!("Notify error: {e}"));
                        if event_sender_clone.send(error_event).is_err() {
                            break; // Receiver dropped
                        }
                    }
                }
            }
        });

        self.event_sender = Some(event_sender);
        self._watcher = Some(watcher);
        self._task_handle = Some(task_handle);

        Ok(event_receiver)
    }

    /// Stop watching all files
    pub async fn stop(&mut self) -> Result<()> {
        // Drop the watcher to stop file watching
        self._watcher = None;

        // Close the event sender to signal the task to exit
        self.event_sender = None;

        // Wait for the task to complete
        if let Some(handle) = self._task_handle.take() {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Convert a notify Event to our WatchEvent
    fn convert_notify_event(event: Event) -> Option<WatchEvent> {
        match event.kind {
            EventKind::Modify(_) => {
                // File was modified
                event.paths.first().map(|path| WatchEvent::FileModified(path.clone()))
            }
            EventKind::Create(_) => {
                // File was created
                event.paths.first().map(|path| WatchEvent::FileCreated(path.clone()))
            }
            EventKind::Remove(_) => {
                // File was deleted
                event.paths.first().map(|path| WatchEvent::FileDeleted(path.clone()))
            }
            _ => {
                // Other event types we don't handle yet
                None
            }
        }
    }

    /// Get the current file state manager
    pub fn file_manager(&self) -> &FileStateManager {
        &self.file_manager
    }

    /// Get mutable access to the file state manager
    pub fn file_manager_mut(&mut self) -> &mut FileStateManager {
        &mut self.file_manager
    }
}

/// Create a new multi-file watcher with default configuration
pub fn create_watcher() -> MultiFileWatcher {
    MultiFileWatcher::new(WatcherConfig::default())
}

/// Create a new multi-file watcher with custom configuration
pub fn create_watcher_with_config(config: WatcherConfig) -> MultiFileWatcher {
    MultiFileWatcher::new(config)
}
