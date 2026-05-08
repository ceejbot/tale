//! Multi-file watching using notify.
//!
//! This module handles file system event monitoring for multiple files,
//! coordinating with the file state manager and batch processor.

use std::path::{Path, PathBuf};

use miette::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::file_state::FileStateManager;
use crate::errors::*;

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

/// Multi-file watcher using notify and async coordination.
///
/// `_watcher` and `_task_handle` are RAII guards: dropping the
/// `MultiFileWatcher` stops the underlying notify watcher and joins the
/// event-processing task. They're never read directly; the underscore prefix
/// signals "kept alive on purpose".
pub struct MultiFileWatcher {
    file_manager: FileStateManager,
    _watcher: Option<RecommendedWatcher>,
    _task_handle: Option<JoinHandle<()>>,
}

impl MultiFileWatcher {
    /// Create a new MultiFileWatcher
    pub fn new() -> Self {
        Self {
            file_manager: FileStateManager::new(),
            _watcher: None,
            _task_handle: None,
        }
    }

    /// Add files to be watched
    pub async fn add_files<I, P>(&mut self, paths: I) -> Result<(), TaleError>
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
    pub async fn watch(&mut self) -> Result<mpsc::UnboundedReceiver<WatchEvent>, TaleError> {
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
                .map_err(TaleError::NotifyError)?;
        }

        // Spawn a task to process notify events and convert them to WatchEvents
        let task_handle = tokio::spawn(async move {
            while let Ok(result) = notify_receiver.recv() {
                match result {
                    Ok(event) => {
                        if let Some(watch_event) = Self::convert_notify_event(event)
                            && event_sender.send(watch_event).is_err()
                        {
                            break; // Receiver dropped
                        }
                    }
                    Err(e) => {
                        let error_event = WatchEvent::Error(format!("Notify error: {e}"));
                        if event_sender.send(error_event).is_err() {
                            break; // Receiver dropped
                        }
                    }
                }
            }
        });

        self._watcher = Some(watcher);
        self._task_handle = Some(task_handle);

        Ok(event_receiver)
    }

    /// Convert a notify Event to our WatchEvent
    fn convert_notify_event(event: Event) -> Option<WatchEvent> {
        match event.kind {
            EventKind::Modify(_) => event.paths.first().map(|path| WatchEvent::FileModified(path.clone())),
            EventKind::Create(_) => event.paths.first().map(|path| WatchEvent::FileCreated(path.clone())),
            EventKind::Remove(_) => event.paths.first().map(|path| WatchEvent::FileDeleted(path.clone())),
            _ => None,
        }
    }

    /// Get mutable access to the file state manager
    pub fn file_manager_mut(&mut self) -> &mut FileStateManager {
        &mut self.file_manager
    }
}

impl Default for MultiFileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a new multi-file watcher with default configuration
pub fn create_watcher() -> MultiFileWatcher {
    MultiFileWatcher::new()
}
