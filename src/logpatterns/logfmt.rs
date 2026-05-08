//! Logformat is not json, but it's a popular way of structuring logs that is
//! fairly easy to parse and display nicely. It emerged from Heroku, which
//! gave us a lot of useful service deployment practices in the form of the
//! 12-factor app. See: <https://brandur.org/logfmt>

use std::collections::HashMap;

use owo_colors::OwoColorize;
use serde::Deserialize;

use crate::logpatterns::PrettyPrintable;
use crate::logpatterns::formatting::*;

#[derive(Debug, Clone)]
pub struct LogfmtLine {
    /// The key=value pairs from the log line log line
    pub pairs: HashMap<String, LogValue>,
    /// Any string content that was not associated with a key
    bare: Vec<String>,
}

impl LogfmtLine {
    pub fn timestamp(&self) -> Option<&jiff::Timestamp> {
        // Try common timestamp field names
        for key in ["timestamp", "ts", "time"] {
            if let Some(LogValue::Timestamp(ts)) = self.pairs.get(key) {
                return Some(ts);
            }
        }
        None
    }

    pub fn level(&self) -> String {
        // Try common level field names
        for key in ["level", "lvl", "severity"] {
            if let Some(LogValue::String(level)) = self.pairs.get(key) {
                return level.clone();
            }
        }
        "INFO".to_string() // Default fallback
    }

    pub fn message(&self) -> String {
        // Try common message field names
        for key in ["message", "msg"] {
            if let Some(LogValue::String(msg)) = self.pairs.get(key) {
                return msg.clone();
            }
        }
        // If no message found, join bare strings
        if !self.bare.is_empty() {
            self.bare.join(" ")
        } else {
            String::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum LogValue {
    String(String),
    Int(u64),
    Float(f64),
    Timestamp(jiff::Timestamp),
}

impl std::fmt::Display for LogValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogValue::String(s) => write!(f, "{}", s),
            LogValue::Int(i) => write!(f, "{}", i),
            LogValue::Float(fl) => write!(f, "{}", fl),
            LogValue::Timestamp(ts) => write!(f, "{}", ts),
        }
    }
}

/// Parse a logfmt line into key-value pairs
fn parse_logfmt(input: &str) -> LogfmtLine {
    let mut pairs = HashMap::new();
    let mut bare = Vec::new();
    let mut chars = input.chars().peekable();

    while chars.peek().is_some() {
        // Skip whitespace
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }

        // Try to parse a key=value pair
        let mut key = String::new();

        // Read until we hit = or whitespace
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(chars.next().expect("we know it has a next"));
        }

        // Check if we have an equals sign
        if chars.peek() == Some(&'=') {
            chars.next(); // consume the '='

            // Parse the value
            let value = if chars.peek() == Some(&'"') {
                // Quoted string value
                chars.next(); // consume opening quote
                let mut val = String::new();
                let mut escaped = false;

                for c in chars.by_ref() {
                    if escaped {
                        val.push(c);
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    } else {
                        val.push(c);
                    }
                }
                val
            } else {
                // Unquoted value - read until whitespace
                let mut val = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    val.push(chars.next().expect("we know it has a next"));
                }
                val
            };

            // Try to parse the value as different types
            let log_value = if let Ok(i) = value.parse::<u64>() {
                LogValue::Int(i)
            } else if let Ok(f) = value.parse::<f64>() {
                LogValue::Float(f)
            } else if let Ok(ts) = jiff::Timestamp::strptime(&value, "%Y-%m-%dT%H:%M:%S%.fZ") {
                LogValue::Timestamp(ts)
            } else if let Ok(ts) = jiff::Timestamp::strptime(&value, "%Y-%m-%d %H:%M:%S") {
                LogValue::Timestamp(ts)
            } else {
                LogValue::String(value)
            };

            pairs.insert(key, log_value);
        } else if !key.is_empty() {
            // We have a bare string (no value)
            bare.push(key);
        }
    }

    LogfmtLine { pairs, bare }
}

impl<'de> Deserialize<'de> for LogfmtLine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // First try to deserialize as a string (for logfmt lines)
        let s = String::deserialize(deserializer)?;
        Ok(parse_logfmt(&s))
    }
}

// special keys: msg, message, level, severity, time, timestamp

impl PrettyPrintable for LogfmtLine {
    fn write(&self, buffer: &mut bytes::BytesMut) -> usize {
        let metrics = LayoutMetrics::current();

        // Write the level column
        let level = self.level();
        buffer.extend_from_slice(get_level_bytes(&level));

        // Write timestamp column if available and show_time is enabled
        if metrics.show_time {
            buffer.extend_from_slice(b" ");
            if let Some(timestamp) = self.timestamp() {
                write_timestamp_column(buffer, timestamp);
            } else {
                write_empty_module_column(buffer);
            }
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        // Format the message
        let message = self.message();
        format_message(&message, buffer, metrics.padding, metrics.max_message_width);

        // Cells for additional key-value pairs
        let cells = self.cells();
        write_cells_with_padding(buffer, &cells, &metrics);

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        // Skip the primary fields we already displayed (level, message, timestamp)
        const SKIP_KEYS: &[&str] = &["level", "lvl", "severity", "message", "msg", "timestamp", "ts", "time"];

        let mut cells = Vec::new();
        for (key, value) in &self.pairs {
            if SKIP_KEYS.contains(&key.as_str()) {
                continue;
            }
            let formatted_value = match value {
                LogValue::String(s) => s.green().to_string(),
                LogValue::Int(i) => i.bright_magenta().to_string(),
                LogValue::Float(f) => f.bright_magenta().to_string(),
                LogValue::Timestamp(ts) => ts.blue().to_string(),
            };
            cells.push(format!("{}={}", key.dimmed(), formatted_value));
        }

        // Add bare strings if any
        for bare_str in &self.bare {
            cells.push(bare_str.cyan().to_string());
        }

        cells
    }
}

impl std::fmt::Display for LogfmtLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}
