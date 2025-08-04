//! File state tracking for multi-file tailing.
//!
//! This module handles tracking file positions, metadata, and state
//! for efficient multi-file tailing operations.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Tracks the state of a single file being tailed
#[derive(Debug, Clone)]
pub struct FileState {
    /// Path to the file
    pub path: PathBuf,
    /// Current byte position in the file
    pub position: u64,
    /// File size when last checked
    pub size: u64,
    /// Inode number for detecting file rotation
    pub inode: Option<u64>,
    /// Whether this file is currently available for reading
    pub available: bool,
}

impl FileState {
    /// Create a new FileState for the given path
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            position: 0,
            size: 0,
            inode: None,
            available: false,
        }
    }

    /// Create a new FileState and immediately refresh it
    pub fn new_and_refresh(path: PathBuf) -> Result<Self> {
        let mut state = Self::new(path);
        state.refresh()?;
        Ok(state)
    }

    /// Set the starting position
    pub fn set_position(&mut self, position: u64) {
        self.position = position;
    }

    /// Update the file state by checking the current file system state
    pub fn refresh(&mut self) -> Result<bool> {
        let metadata = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                // File doesn't exist
                let was_available = self.available;
                self.available = false;
                self.size = 0;
                self.inode = None;
                return Ok(was_available); // Changed if it was previously available
            }
            Err(err) => {
                return Err(err).with_context(|| format!("Failed to get metadata for {}", self.path.display()));
            }
        };

        let new_size = metadata.len();

        #[cfg(unix)]
        let new_inode = Some(metadata.ino());
        #[cfg(not(unix))]
        let new_inode = None;

        let was_available = self.available;
        let old_inode = self.inode;
        let old_size = self.size;

        // We only want to switch to the new inode if the `sticky` option was set, aka
        // -F. If it's set, we open the new file at our followed path.
        // Otherwise, we keep following the inode we were following before,
        // under whatever its new name is.
        let file_rotated = if old_inode.is_some() && new_inode != old_inode {
            if crate::config::sticky() {
                self.inode = new_inode;
                self.position = 0;
                // The metadata we just read applies to the new inode we're following.
                self.available = metadata.is_file();
                self.size = new_size;

                true
            } else {
                // I *think* the right behavior here is to close us on out?
                self.available = false;
                self.size = 0;
                self.inode = None;
                true
            }
        } else {
            // No rotation: update normal file state
            self.available = metadata.is_file();
            self.size = new_size;
            self.inode = new_inode;
            false
        };

        // Changes include such changes as: availability, rotation, size change, an
        // almost fanati--
        let changed = was_available != self.available || file_rotated || (self.available && new_size > old_size);

        Ok(changed)
    }

    /// Check if this file has new data available since last read
    pub fn has_new_data(&self) -> bool {
        self.available && self.size > self.position
    }

    /// Mark that we've read up to the given position
    pub fn update_position(&mut self, new_position: u64) {
        self.position = new_position;
    }

    /// Read new lines from the file starting from the current position
    pub fn read_new_lines(&mut self) -> Result<Vec<String>> {
        if !self.available || !self.has_new_data() {
            return Ok(Vec::new());
        }

        let mut file =
            File::open(&self.path).with_context(|| format!("Failed to open file: {}", self.path.display()))?;

        // Seek to our current position
        file.seek(SeekFrom::Start(self.position)).with_context(|| {
            format!(
                "Failed to seek to position {} in {}",
                self.position,
                self.path.display()
            )
        })?;

        let mut reader = BufReader::new(file);
        let mut lines = Vec::new();

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line)? {
                0 => break, // EOF reached
                bytes_read => {
                    // Remove trailing newline if present
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }

                    lines.push(line);
                    self.position += bytes_read as u64;
                }
            }
        }

        Ok(lines)
    }
}

/// Manager for tracking multiple file states
#[derive(Debug)]
pub struct FileStateManager {
    /// Map of file path to its state
    states: HashMap<PathBuf, FileState>,
}

impl FileStateManager {
    /// Create a new FileStateManager
    pub fn new() -> Self {
        Self { states: HashMap::new() }
    }

    /// Add a file to be tracked
    pub fn add_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let state = FileState::new_and_refresh(path.clone())?;
        self.states.insert(path, state);
        Ok(())
    }

    /// Add a file to be tracked, starting from the end. No offsets for
    /// MULTIBALL.
    pub fn add_file_for_tailing<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let mut state = FileState::new_and_refresh(path.clone())?;
        state.set_position(state.size);
        self.states.insert(path, state);
        Ok(())
    }

    /// Stop tailing this file.
    pub fn remove_file<P: AsRef<Path>>(&mut self, path: P) {
        self.states.remove(path.as_ref());
    }

    /// Get the state for a specific file
    pub fn get_state<P: AsRef<Path>>(&self, path: P) -> Option<&FileState> {
        self.states.get(path.as_ref())
    }

    /// Get mutable state for a specific file
    pub fn get_state_mut<P: AsRef<Path>>(&mut self, path: P) -> Option<&mut FileState> {
        self.states.get_mut(path.as_ref())
    }

    /// Refresh all file states
    pub fn refresh_all(&mut self) -> Result<Vec<PathBuf>> {
        let mut changed_files = Vec::new();

        for (path, state) in &mut self.states {
            if let Ok(changed) = state.refresh() {
                if changed {
                    changed_files.push(path.clone());
                }
            }
        }

        Ok(changed_files)
    }

    /// Get all files that have new data available
    pub fn files_with_new_data(&self) -> Vec<&PathBuf> {
        self.states
            .iter()
            .filter_map(|(path, state)| if state.has_new_data() { Some(path) } else { None })
            .collect()
    }

    /// Get all tracked file paths
    pub fn tracked_files(&self) -> Vec<&PathBuf> {
        self.states.keys().collect()
    }

    /// Read new lines from all files that have new data
    pub fn read_new_lines(&mut self) -> Result<Vec<(PathBuf, Vec<String>)>> {
        let mut all_new_lines = Vec::new();

        for (path, state) in &mut self.states {
            if state.has_new_data() {
                let lines = state.read_new_lines()?;
                if !lines.is_empty() {
                    all_new_lines.push((path.clone(), lines));
                }
            }
        }

        Ok(all_new_lines)
    }

    /// Read all lines from all tracked files (for static multi-file reading)
    pub fn read_all_lines(&mut self) -> Result<Vec<(PathBuf, Vec<String>)>> {
        let mut all_lines = Vec::new();

        for (path, state) in &self.states {
            if state.available {
                let lines = Self::read_all_lines_from_file(state)?;
                if !lines.is_empty() {
                    all_lines.push((path.clone(), lines));
                }
            }
        }

        Ok(all_lines)
    }

    /// Read all lines from a specific file from the beginning
    fn read_all_lines_from_file(state: &FileState) -> Result<Vec<String>> {
        if !state.available {
            return Ok(Vec::new());
        }

        let mut file =
            File::open(&state.path).with_context(|| format!("Failed to open file: {}", state.path.display()))?;

        // Always start from the beginning for static reading
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("Failed to seek to beginning of {}", state.path.display()))?;

        let reader = BufReader::new(file);
        let mut lines = Vec::new();

        for line_result in reader.lines() {
            let line = line_result.with_context(|| format!("Failed to read line from {}", state.path.display()))?;
            lines.push(line);
        }

        Ok(lines)
    }
}

impl Default for FileStateManager {
    fn default() -> Self {
        Self::new()
    }
}
