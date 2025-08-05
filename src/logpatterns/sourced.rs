//! SourcedLine is a wrapper for a printable line that keeps track of
//! where it came from. It's my solution at the moment for keeping track
//! of the information I need to multiplex log lines while printing from
//! more than one file at a time, and to keep them roughly sorted.
//! I might have overengineered this and solved a problem that doesn't
//! matter, so this might vanish or shrink.

use std::fmt::Display;
use std::path::PathBuf;

use bytes::BytesMut;

use super::{PrettyPrintable, Printable};

/// A wrapper that combines a parsed log line with source file metadata.
/// This is used for multi-file processing to track which file each line came
/// from.
#[derive(Debug, Clone)]
pub struct SourcedLine<'a> {
    /// The parsed log line
    pub parsed: Printable<'a>,
    /// The source file path
    pub source_file: PathBuf,
    /// Line number within the source file (0-based)
    pub line_number: usize,
}

impl<'a> SourcedLine<'a> {
    /// Create a new SourcedLine
    pub fn new(parsed: Printable<'a>, source_file: PathBuf, line_number: usize) -> Self {
        Self {
            parsed,
            source_file,
            line_number,
        }
    }

    /// Get the source file name (without path) for display
    pub fn source_file_name(&self) -> &str {
        self.source_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    }

    /// Extract timestamp from the parsed line, if available
    pub fn timestamp(&self) -> Option<&jiff::Timestamp> {
        match &self.parsed {
            Printable::Canonical(canonical) => Some(&canonical.timestamp),
            Printable::Java(java) => Some(&java.timestamp),
            Printable::Message(message) => message.timestamp.as_ref(),
            Printable::TimeOnly(timestamped) => Some(&timestamped.timestamp),
            Printable::Json(_) => None,
            Printable::Text(_) => None,
        }
    }

    /// Get the sort key for multi-file chronological ordering
    pub fn sort_key(&self) -> SortKey {
        if let Some(ts) = self.timestamp() {
            // If we have a timestamp, use it for sorting
            SortKey::Timestamp(ts.clone())
        } else {
            // If no timestamp, sort by file path and line number to maintain file order
            SortKey::FileOrder {
                file: self.source_file.clone(),
                line: self.line_number,
            }
        }
    }
}

impl<'a> From<(PathBuf, usize, &'a str)> for SourcedLine<'a> {
    fn from(value: (PathBuf, usize, &'a str)) -> Self {
        let parsed = match serde_json::from_str::<Printable<'a>>(value.2) {
            Ok(v) => v,
            Err(_) => Printable::Text(value.2.to_owned()),
        };
        Self {
            parsed,
            source_file: value.0,
            line_number: value.1,
        }
    }
}

/// Key used for sorting multi-file lines chronologically while preserving file
/// order for non-timestamped lines
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// Sort by extracted timestamp (highest priority)
    Timestamp(jiff::Timestamp),
    /// Sort by file path and line number for non-timestamped lines (lower
    /// priority)
    FileOrder { file: PathBuf, line: usize },
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            // Timestamp vs Timestamp: chronological order
            (SortKey::Timestamp(a), SortKey::Timestamp(b)) => a.cmp(b),

            // FileOrder vs FileOrder: file path, then line number
            (SortKey::FileOrder { file: f1, line: l1 }, SortKey::FileOrder { file: f2, line: l2 }) => {
                f1.cmp(f2).then(l1.cmp(l2))
            }

            // Mixed: Need to be smart about this
            (SortKey::Timestamp(_), SortKey::FileOrder { .. }) => {
                // Timestamped lines get sorted chronologically
                // Non-timestamped lines are assumed to be "now" relative to their position
                // This is tricky - for now, timestamped lines come first
                Ordering::Less
            }

            (SortKey::FileOrder { .. }, SortKey::Timestamp(_)) => {
                // Reverse of above
                Ordering::Greater
            }
        }
    }
}

impl<'a> PrettyPrintable for SourcedLine<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        // For now, just delegate to the wrapped Printable
        // TODO: Add source file display based on config flags
        self.parsed.write(buffer)
    }
    fn cells(&self) -> Vec<String> {
        self.parsed.cells()
    }
}

impl<'a> Display for SourcedLine<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // For now, just delegate to the wrapped Printable
        self.parsed.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    // test helper
    fn extract_msg(item: &SourcedLine<'_>) -> String {
        match item.parsed {
            Printable::Canonical(ref canonical) => canonical.message.as_ref().to_owned(),
            Printable::Java(ref java) => java.message.as_ref().to_owned(),
            Printable::Message(ref message) => message.message.as_ref().to_owned(),
            Printable::TimeOnly(ref timestamped) => {
                // for these tests, the following is true, and it's reasonable to make these
                // assertions.
                let obj = timestamped.rest.as_object().expect("rest should be a json object");
                let message = obj.get("message").expect("there is a message in this bottle yeah-a");
                message.as_str().unwrap_or_default().to_string()
            }
            Printable::Json(ref generic_json) => {
                // Handle JSON that didn't match other patterns - try to extract message field
                if let Some(obj) = generic_json.rest.as_object() {
                    if let Some(message) = obj.get("message") {
                        return message.as_str().unwrap_or_default().to_string();
                    }
                }
                String::default()
            }
            Printable::Text(ref v) => v.clone(),
        }
    }

    #[test]
    fn chronological_sort_across_files() {
        use std::path::PathBuf;

        // Lines from different files with different timestamps
        let lines = vec![
            (
                PathBuf::from("file1.log"),
                0,
                r#"{"timestamp": "2025-08-01T10:02:00Z", "message": "file1 line1"}"#.to_string(),
            ),
            (
                PathBuf::from("file2.log"),
                0,
                r#"{"timestamp": "2025-08-01T10:01:00Z", "message": "file2 line1"}"#.to_string(),
            ),
            (
                PathBuf::from("file1.log"),
                1,
                r#"{"timestamp": "2025-08-01T10:03:00Z", "message": "file1 line2"}"#.to_string(),
            ),
            (
                PathBuf::from("file2.log"),
                1,
                r#"{"timestamp": "2025-08-01T10:00:30Z", "message": "file2 line2"}"#.to_string(),
            ),
        ];

        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Should be sorted chronologically regardless of file
        assert_eq!(extract_msg(&sorted[0]), "file2 line2");
        assert_eq!(extract_msg(&sorted[1]), "file2 line1");
        assert_eq!(extract_msg(&sorted[2]), "file1 line1");
        assert_eq!(extract_msg(&sorted[3]), "file1 line2");
    }

    #[test]
    fn sorting_mixed_types() {
        use std::path::PathBuf;

        // Mix of timestamped and non-timestamped lines
        let lines = vec![
            (
                PathBuf::from("file1.log"),
                0,
                r#"{"message": "no timestamp 1"}"#.to_string(),
            ),
            (
                PathBuf::from("file2.log"),
                0,
                r#"{"timestamp": "2025-08-01T10:01:00Z", "message": "timestamped"}"#.to_string(),
            ),
            (
                PathBuf::from("file1.log"),
                1,
                r#"{"message": "no timestamp 2"}"#.to_string(),
            ),
        ];

        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Timestamped lines should come first, then non-timestamped in file order
        assert_eq!(extract_msg(&sorted[0]), "timestamped");
        assert_eq!(extract_msg(&sorted[1]), "no timestamp 1");
        assert_eq!(extract_msg(&sorted[2]), "no timestamp 2");
    }

    #[test]
    fn sorting_preserves_file_order() {
        use std::path::PathBuf;

        // Non-timestamped lines from multiple files
        let lines = vec![
            (PathBuf::from("b.log"), 1, r#"{"message": "b file line 2"}"#.to_string()),
            (PathBuf::from("a.log"), 0, r#"{"message": "a file line 1"}"#.to_string()),
            (PathBuf::from("b.log"), 0, r#"{"message": "b file line 1"}"#.to_string()),
            (PathBuf::from("a.log"), 1, r#"{"message": "a file line 2"}"#.to_string()),
        ];

        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Should be sorted by file path, then line number
        assert_eq!(extract_msg(&sorted[0]), "a file line 1");
        assert_eq!(extract_msg(&sorted[1]), "a file line 2");
        assert_eq!(extract_msg(&sorted[2]), "b file line 1");
        assert_eq!(extract_msg(&sorted[3]), "b file line 2");
    }

    #[test]
    fn sorting_handle_empty_input() {
        let mut lines: Vec<SourcedLine<'_>> = vec![];
        lines.sort_by_key(|xs| xs.sort_key());
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn sorting_handles_invalid_json() {
        use std::path::PathBuf;

        // Invalid JSON should be treated as non-timestamped
        let lines = vec![
            (PathBuf::from("test.log"), 0, "not json at all".to_string()),
            (
                PathBuf::from("test.log"),
                1,
                r#"{"timestamp": "2025-08-01T10:01:00Z", "message": "valid"}"#.to_string(),
            ),
        ];

        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Valid timestamped line should come first
        assert_eq!(extract_msg(&sorted[0]), "valid");
        assert_eq!(extract_msg(&sorted[1]), "not json at all");
    }
}
