//! Custom error types for tale with rich diagnostic information

use std::path::{Path, PathBuf};

use miette::{Diagnostic, ErrReport, SourceSpan};
use owo_colors::OwoColorize;
use thiserror::Error;

use crate::batch::BatchedLine;

/// Main error type for tale operations
#[derive(Error, Debug, Diagnostic)]
pub enum TaleError {
    /// File-related errors with helpful context
    #[error("File operation failed")]
    #[diagnostic()]
    File(#[from] Box<FileError>),

    /// JSON parsing errors with location information
    #[error("JSON format error")]
    #[diagnostic()]
    Json(#[from] Box<JsonError>),

    /// Configuration errors
    #[error("Configuration error")]
    #[diagnostic()]
    Config(#[from] Box<ConfigError>),

    /// I/O errors with context
    #[error("I/O operation failed")]
    #[diagnostic()]
    Io(#[from] Box<IoError>),

    /// Memory management errors
    #[error("Memory management error: {0}")]
    #[diagnostic(code(tale::memory::error))]
    MemoryError(String),

    /// Globular errors.
    #[error(transparent)]
    #[diagnostic()]
    GlobPattern(#[from] glob::PatternError),

    #[error(transparent)]
    #[diagnostic()]
    BatchedLineSendVec(#[from] tokio::sync::mpsc::error::SendError<Vec<BatchedLine>>),

    #[error(transparent)]
    #[diagnostic()]
    BatchedLineSendSingle(#[from] tokio::sync::mpsc::error::SendError<BatchedLine>),

    #[error("Internal error: failed to set up async channels for lines")]
    LineReceiver,

    #[error("Internal error: failed to set up async channels")]
    BatchSender,

    #[error(transparent)]
    #[diagnostic()]
    NotifyError(#[from] notify::Error),
}

/// File-related error types
#[derive(Error, Debug, Diagnostic)]
pub enum FileError {
    /// File not found with suggestions
    #[error("File not found: {}", path.display().yellow().bold())]
    #[diagnostic(code(tale::file::not_found))]
    NotFound { path: PathBuf, similar_files: Vec<String> },

    /// Permission denied with actionable advice
    #[error("Permission denied: {}", path.display().yellow().bold())]
    #[diagnostic(code(tale::file::permission_denied))]
    PermissionDenied { path: PathBuf, suggestion: String },

    /// Not a file (directory, device, etc.)
    #[error("Not a regular file: {} is a {}", path.display().yellow().bold(), actual_type.bold())]
    #[diagnostic(code(tale::file::not_a_file))]
    NotAFile { path: PathBuf, actual_type: String },
}

/// JSON parsing error types
#[derive(Error, Debug, Diagnostic)]
pub enum JsonError {
    /// Invalid JSON syntax
    #[error("Invalid JSON syntax")]
    #[diagnostic(code(tale::json::invalid_syntax))]
    InvalidSyntax {
        #[source_code]
        src: String,
        #[label("invalid JSON here")]
        span: SourceSpan,
        details: String,
    },

    /// Missing required field
    #[error("Missing required field: {}", field.blue())]
    #[diagnostic(code(tale::json::missing_field))]
    MissingField {
        field: String,
        #[source_code]
        src: String,
        #[label("in this JSON object")]
        span: SourceSpan,
    },
}

/// Configuration error types
#[derive(Error, Debug, Diagnostic)]
pub enum ConfigError {
    /// Invalid argument combination
    #[error("Invalid argument combination")]
    #[diagnostic(code(tale::config::invalid_args))]
    InvalidArgs {
        message: String,
        conflicting_args: Vec<String>,
    },

    /// Invalid offset value
    #[error("Invalid offset value: {}", value.blue())]
    #[diagnostic(code(tale::config::invalid_offset))]
    InvalidOffset { value: String, reason: String },
}

/// I/O error wrapper with context
#[derive(Error, Debug, Diagnostic)]
pub enum IoError {
    /// Generic I/O error with context
    #[error("I/O error: {}", operation.blue())]
    #[diagnostic(code(tale::io::operation_failed))]
    OperationFailed {
        operation: String,
        path: Option<PathBuf>,
        #[source]
        source: std::io::Error,
    },
}

impl From<ErrReport> for TaleError {
    fn from(_value: ErrReport) -> Self {
        todo!()
    }
}

// Helper functions for creating errors with context
impl FileError {
    /// Create a file not found error with suggestions
    pub fn not_found_with_suggestions(path: PathBuf, similar_files: Vec<PathBuf>) -> Self {
        let similar_files = similar_files.into_iter().map(|p| p.display().to_string()).collect();
        Self::NotFound { path, similar_files }
    }

    /// Create a permission denied error with helpful context
    pub fn permission_denied_with_suggestion(path: PathBuf, suggestion: Option<String>) -> Self {
        let suggestion = suggestion.unwrap_or_else(|| "Check file permissions".to_string());
        Self::PermissionDenied { path, suggestion }
    }

    /// Create a not-a-file error with type information
    pub fn not_a_file_with_type(path: PathBuf) -> Self {
        let actual_type = if path.is_dir() {
            "directory".to_string()
        } else if path.is_symlink() {
            "symbolic link".to_string()
        } else {
            "special file".to_string()
        };

        Self::NotAFile { path, actual_type }
    }
}

impl JsonError {
    /// Create a JSON syntax error with source location
    pub fn invalid_syntax_at(src: String, offset: usize, len: usize, details: String) -> Self {
        Self::InvalidSyntax {
            src,
            span: SourceSpan::new(offset.into(), len),
            details,
        }
    }
}

/// Helper trait for converting std::io::Error to our IoError with context
pub trait IoErrorExt<T> {
    fn with_context(self, operation: &str, path: Option<&Path>) -> Result<T, IoError>;
}

impl<T> IoErrorExt<T> for Result<T, std::io::Error> {
    fn with_context(self, operation: &str, path: Option<&Path>) -> Result<T, IoError> {
        self.map_err(|e| IoError::OperationFailed {
            operation: operation.to_string(),
            path: path.map(|p| p.to_path_buf()),
            source: e,
        })
    }
}

/// Automatic conversion from std::io::Error to our TaleError for easier use
/// with ?
impl From<std::io::Error> for TaleError {
    fn from(err: std::io::Error) -> Self {
        TaleError::Io(Box::new(IoError::OperationFailed {
            operation: "I/O operation".to_string(),
            path: None,
            source: err,
        }))
    }
}

/// Automatic conversion from serde_json::Error to our TaleError for easier use
/// with ?
impl From<serde_json::Error> for TaleError {
    fn from(err: serde_json::Error) -> Self {
        TaleError::Json(Box::new(JsonError::InvalidSyntax {
            src: "JSON input".to_string(),
            span: miette::SourceSpan::new(0.into(), 0usize),
            details: err.to_string(),
        }))
    }
}

/// Helper for finding similar file names
pub fn find_similar_files(target: &Path) -> Vec<PathBuf> {
    let Some(parent) = target.parent() else {
        return Vec::new();
    };

    let Some(target_name) = target.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut similar = Vec::new();

    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if name != target_name && is_similar(target_name, name) {
                similar.push(entry.path());
            }
        }
    }

    // Sort by similarity/distance
    similar.sort_by(|a, b| {
        let a_name = a.file_name().unwrap_or_default().to_str().unwrap_or_default();
        let b_name = b.file_name().unwrap_or_default().to_str().unwrap_or_default();
        let a_dist = edit_distance(target_name, a_name);
        let b_dist = edit_distance(target_name, b_name);
        a_dist.cmp(&b_dist)
    });

    similar.truncate(3); // Show at most 3 suggestions
    similar
}

/// Simple similarity check (case-insensitive, simple character differences)
fn is_similar(target: &str, candidate: &str) -> bool {
    let target = target.to_lowercase();
    let candidate = candidate.to_lowercase();

    // Check for simple typos or file extension changes (1-3 character differences)
    let distance = edit_distance(&target, &candidate);
    distance <= 3 && distance > 0
}

/// Simple edit distance calculation
fn edit_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.len();
    let len2 = s2.len();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        matrix[i][0] = i;
    }
    for j in 0..=len2 {
        matrix[0][j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1.chars().nth(i - 1) == s2.chars().nth(j - 1) {
                0
            } else {
                1
            };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(matrix[i - 1][j] + 1, matrix[i][j - 1] + 1),
                matrix[i - 1][j - 1] + cost,
            );
        }
    }

    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("hello", "hello"), 0);
        assert_eq!(edit_distance("hello", "helo"), 1);
        assert_eq!(edit_distance("hello", "help"), 2);
        assert_eq!(edit_distance("file.txt", "file.log"), 3);
    }

    #[test]
    fn test_similarity() {
        assert!(is_similar("file.txt", "file.log"));
        assert!(is_similar("config.json", "config.jsn"));
        assert!(!is_similar("file.txt", "completely_different.py"));
    }

    #[test]
    fn test_file_error_creation() {
        let path = PathBuf::from("/nonexistent/file.txt");
        let similar = vec![PathBuf::from("/nonexistent/file.log")];

        let error = FileError::not_found_with_suggestions(path.clone(), similar);

        match error {
            FileError::NotFound {
                path: error_path,
                similar_files: suggestions,
            } => {
                assert_eq!(error_path, path);
                assert!(!suggestions.is_empty());
                assert_eq!(suggestions.len(), 1);
            }
            _ => panic!("Wrong error type created"),
        }
    }
}
