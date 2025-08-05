//! The logging patterns themselves, with implementations.
//! They have to be deserialized from most specific to least
//! specific but they're in the opposite order in this file.

use std::borrow::Cow;
use std::fmt::Display;

use ansi_width::ansi_width;
use bytes::{BufMut, BytesMut};
use humansize::{BINARY, format_size};
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;
use textwrap::termwidth;

use super::formatting::*;
use super::{PrettyPrintable, columns};
use crate::config;

/// We couldn't parse this json as a log line, even minimally, but it's
/// json so we'll print it.
#[derive(Debug, Clone, Deserialize)]
pub struct GenericJson {
    #[serde(flatten)]
    pub(super) rest: serde_json::Value,
}

impl PrettyPrintable for &GenericJson {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = config::show_time();
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };

        buffer.extend_from_slice(JSON_HEADER);
        if show_time {
            buffer.extend_from_slice(b"                    ");
            buffer.extend_from_slice(COL_SEP.as_bytes());
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        write_rest(*self, buffer, max_message_width, padding)
    }

    fn cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = Vec::new();
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            _ => {
                cells.push(colorize_map_entry("rest", &self.rest));
            }
        }
        cells
    }
}

impl Display for GenericJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer.utf8_chunks().try_for_each(|chunk| {
            let c = chunk.valid();
            write!(f, "{c}")
        })
    }
}

/// The *only* thing we could recognize here was a timestamp.
#[derive(Debug, Clone, Deserialize)]
pub struct Timestamped {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    /// Everything else.
    #[serde(flatten)]
    pub(super) rest: serde_json::Value,
}

impl Display for Timestamped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer.utf8_chunks().try_for_each(|chunk| {
            let c = chunk.valid();
            write!(f, "{c}")
        })
    }
}

impl PrettyPrintable for &Timestamped {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = config::show_time();
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };

        buffer.extend_from_slice(JSON_HEADER);
        if show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        write_rest(*self, buffer, max_message_width, padding)
    }

    fn cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = Vec::new();
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            _ => {
                cells.push(colorize_map_entry("rest", &self.rest));
            }
        }
        cells
    }
}

/// A log line, somewhat flexibly formatted. There are two fields that we demand
/// be present: a log level and a message. We allow several possible names for
/// common log fields. The rest of the fields are mentioned only so we can print
/// them in a controlled order. We do not demand that they be present.
#[derive(Debug, Clone, Deserialize)]
pub struct Message<'a> {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(crate) timestamp: Option<jiff::Timestamp>,
    /// The string message part of the log line.
    #[serde(alias = "msg", borrow)]
    pub(crate) message: Cow<'a, str>,
    /// Log level for this line.
    #[serde(alias = "lvl", alias = "severity", borrow)]
    pub(crate) level: Cow<'a, str>,
    /// The source module where the log line originated.
    #[serde(alias = "mod", alias = "lib", borrow)]
    pub(crate) module: Option<Cow<'a, str>>,
    /// The file where the log line originated.
    #[serde(borrow)]
    pub(crate) file: Option<Cow<'a, str>>,
    /// The line in the file where the log line originated.
    pub(crate) line: Option<usize>,
    /// A request id
    #[serde(alias = "requestId", borrow)]
    pub(crate) request_id: Option<Cow<'a, str>>,
    /// Remote host.
    #[serde(alias = "hostname", alias = "remote_host", borrow)]
    pub(crate) host: Option<Cow<'a, str>>,
    /// http method
    #[serde(borrow)]
    pub(crate) method: Option<Cow<'a, str>>,
    /// a url
    #[serde(borrow)]
    pub(crate) url: Option<Cow<'a, str>>,
    /// http response status code
    #[serde(alias = "statusCode", alias = "code", borrow)]
    pub(crate) status: Option<Cow<'a, str>>,
    /// elapsed time
    #[serde(alias = "elapsed_ms", alias = "elapsed_time", borrow)]
    pub(crate) elapsed: Option<Cow<'a, str>>,
    /// size of the written response
    #[serde(
        alias = "sent_bytes",
        alias = "length",
        alias = "written",
        alias = "bodylen",
        alias = "body",
        alias = "size",
        borrow
    )]
    pub(crate) size: Option<Cow<'a, str>>,
    /// The unpredictable parts of the log line, which we'll handle more
    /// generically.
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
}

impl<'a> Display for Message<'a> {
    // second verse, same as the first
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer.utf8_chunks().try_for_each(|chunk| {
            let c = chunk.valid();
            write!(f, "{c}")
        })
    }
}

impl<'a> PrettyPrintable for &Message<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = config::show_time();
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };
        // Let's get absurd! Who wants to alloc, anyway?
        buffer.extend_from_slice(get_level_bytes(&self.level));

        if show_time {
            if let Some(ref v) = self.timestamp {
                // Add space + formatted timestamp + separator
                buffer.extend_from_slice(b" ");
                write_timestamp_column(buffer, v);
            } else if let Some(ref v) = self.request_id {
                // Add space + formatted request_id + separator
                buffer.extend_from_slice(b" ");
                write_request_id_column(buffer, v);
            } else {
                // Add space + padding + separator
                buffer.extend_from_slice(b" ");
                write_empty_module_column(buffer);
            }
        } else {
            // Just add the separator
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, padding, max_message_width);

        // Now we walk through all the other fields, treating some specially,
        // building our list of cells somewhat inefficiently. We pad everything
        // out to the ideal column width as we go, to avoid having to work to
        // avoid counting ansi escapes.
        let mut cells: Vec<String> = Vec::new();

        // Work a little bit to show the log line location coherently.
        // Probably if one is present, all are present, but not always.
        let logloc = if let Some(ref module) = self.module
            && let Some(ref fname) = self.file
            && let Some(lineno) = self.line
        {
            format!("{module}: {fname}:{lineno}")
        } else if let Some(ref fname) = self.file
            && let Some(lineno) = self.line
        {
            format!("{fname}:{lineno}")
        } else if let Some(ref fname) = self.file {
            fname.to_string()
        } else {
            Default::default()
        };
        if !logloc.is_empty() {
            cells.push(format!("{}", logloc.yellow()));
        }

        // if we didn't already snag the request id
        if !show_time && let Some(ref v) = self.request_id {
            cells.push(format!("{}{}", "reqid=".dimmed(), v.bright_yellow()));
        }

        let status = if let Some(ref v) = self.status {
            // TODO transform this into `200 OK` or similar canonical form
            v.to_string()
        } else {
            String::default()
        };

        // Special treatment for verb url status
        let req_line = if let Some(ref method) = self.method
            && let Some(ref url) = self.url
        {
            format!("{method} {url} {status}")
        } else if let Some(ref url) = self.url {
            format!("{url}{status}")
        } else {
            status
        };
        if !req_line.is_empty() {
            cells.push(format!("{}", req_line.blue()));
        }

        if let Some(ref v) = self.host {
            cells.push(format!("{}{}", "host=".dimmed(), v.blue()));
        }

        // Units and format are unpredictable, so we leave it as-is.
        if let Some(ref v) = self.elapsed {
            cells.push(format!("{}{}", "elapsed=".dimmed(), v.bright_purple()));
        }

        // Same as above.
        if let Some(ref v) = self.size {
            cells.push(format!("{}{}", "size=".dimmed(), v.bright_purple()));
        }
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            _ => {
                cells.push(colorize_map_entry("rest", &self.rest));
            }
        }

        // Write the columns with proper padding for continuation lines
        if !cells.is_empty() {
            let mut column_buffer = BytesMut::new();
            columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

            let column_output = String::from_utf8_lossy(&column_buffer);
            for line in column_output.lines() {
                if !line.trim().is_empty() {
                    // Add padding
                    for _ in 0..padding {
                        buffer.extend_from_slice(b" ");
                    }
                    buffer.extend_from_slice(COL_SEP.as_bytes());
                    buffer.extend_from_slice(line.as_bytes());
                    buffer.extend_from_slice(b"\n");
                }
            }
        }

        // if it is this easy I can get rid of the return value
        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = Vec::new();
        match &self.rest {
            Value::Object(map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            v => {
                cells.push(colorize_json_value(&v));
            }
        }
        cells
    }
}

/// This is something log4j or slf4j might emit. One of those.
#[derive(Debug, Clone, Deserialize)]
pub struct Java<'a> {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    #[serde(alias = "msg")]
    pub(super) message: Cow<'a, str>,
    #[serde(alias = "severity")]
    pub(super) level: Cow<'a, str>,
    #[serde(alias = "threadName")]
    thread: Cow<'a, str>,
    #[serde(alias = "requestid", alias = "requestId")]
    request_id: Cow<'a, str>,
    class: Cow<'a, str>,
    method: Cow<'a, str>,
    source: Cow<'a, str>,
    #[serde(rename = "stackTrace")]
    stack_trace: Cow<'a, str>,
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
}

impl<'a> PrettyPrintable for Java<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = config::show_time();
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };

        let mut pad_or_newline = |width: usize, count: usize, buf: &mut BytesMut| {
            if count + width + 2 >= max_message_width {
                start_new_line(buf, padding);
                padding + 3
            } else {
                buf.extend_from_slice(b"  ");
                count + 2
            }
        };

        buffer.extend_from_slice(get_level_bytes(&self.level));
        if show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, padding, max_message_width);

        for _ in 0..padding {
            buffer.extend_from_slice(b" ");
        }
        buffer.extend_from_slice(COL_SEP.as_bytes());
        let mut count = padding + 3;

        if !self.request_id.is_empty() {
            buffer.extend_from_slice(self.request_id.bright_yellow().to_string().as_bytes());
            count += self.request_id.len();
        }

        let mut formatted: String;
        if !self.thread.is_empty() {
            formatted = format!("{}{}", "thread=".dimmed(), self.thread.green());
            let nextwidth = ansi_width(&formatted);
            if !self.request_id.is_empty() {
                count = pad_or_newline(nextwidth, count, buffer);
            }
            buffer.extend_from_slice(formatted.as_bytes());
            count += nextwidth;
        }

        formatted = format!("{} :: {}", self.class.blue(), self.method.blue());
        let nextwidth = ansi_width(&formatted);
        count = pad_or_newline(nextwidth, count, buffer);
        buffer.extend_from_slice(formatted.as_bytes());
        count += nextwidth;

        formatted = format!("{}", self.source.purple());
        let nextwidth = ansi_width(&formatted);
        pad_or_newline(nextwidth, count, buffer);
        buffer.extend_from_slice(formatted.as_bytes());
        // count += nextwidth;

        let cells = self.cells();
        buffer.extend_from_slice(b"\n");
        if cells.is_empty() {
            return buffer.len();
        }

        // Write the columns with proper padding for continuation lines
        let mut column_buffer = BytesMut::new();
        columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

        let column_output = String::from_utf8_lossy(&column_buffer);
        for line in column_output.lines() {
            if !line.trim().is_empty() {
                // Add padding
                for _ in 0..padding {
                    buffer.extend_from_slice(b" ");
                }
                buffer.extend_from_slice(COL_SEP.as_bytes());
                buffer.extend_from_slice(line.as_bytes());
                buffer.extend_from_slice(b"\n");
            }
        }

        if !self.stack_trace.is_empty() {
            // start a new line, with separator
            for _ in 0..padding {
                buffer.extend_from_slice(b" ");
            }
            buffer.extend_from_slice(COL_SEP.as_bytes());
            // We treat this like a message and either emit intact or wrap it.
            format_message(&self.stack_trace, buffer, padding, max_message_width);
        }

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = Vec::new();
        match &self.rest {
            Value::Object(map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            v => {
                cells.push(colorize_json_value(&v));
            }
        }
        cells
    }
}

impl<'a> Display for Java<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer.utf8_chunks().try_for_each(|chunk| {
            let c = chunk.valid();
            write!(f, "{c}")
        })
    }
}

/// This is a possibly familiar format that we require conformance to.
#[derive(Debug, Clone, Deserialize)]
pub struct Canonical<'a> {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    #[serde(alias = "severity")]
    pub(super) level: Cow<'a, str>,
    pub(super) message: Cow<'a, str>,
    method: Cow<'a, str>,
    url: Cow<'a, str>,
    status: usize,
    elapsed: Cow<'a, str>,
    size: usize,
    request_id: Cow<'a, str>,
    remote_host: Cow<'a, str>,
    user_agent: Cow<'a, str>,
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
}

impl<'a> PrettyPrintable for &Canonical<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = config::show_time();
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };

        buffer.extend_from_slice(get_level_bytes(&self.level));
        if show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, padding, max_message_width);

        for _ in 0..padding {
            buffer.extend_from_slice(b" ");
        }
        buffer.extend_from_slice(COL_SEP.as_bytes());
        let mut count = padding + 3;
        buffer.extend_from_slice(self.request_id.bright_yellow().to_string().as_bytes());
        buffer.extend_from_slice(b"  ");
        count += self.request_id.len() + 2;
        let mut formatted = format!("{} {} {} ", self.method.blue(), self.url.blue(), self.status.blue());
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);
        formatted.clear();

        let sized = format_size(self.size, BINARY);
        formatted = format!("{}{}  ", "size=".dimmed(), sized.bright_magenta());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        // tedious repetition but
        formatted = format!("{}{}  ", "elapsed=".dimmed(), self.elapsed.magenta());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        formatted = format!("{}{}  ", "remote_host=".dimmed(), self.remote_host.blue());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        formatted = format!("{}{}  ", "user_agent=".dimmed(), self.user_agent.green());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
        }
        buffer.extend_from_slice(formatted.as_bytes());

        let cells = self.cells();
        buffer.extend_from_slice(b"\n");
        if cells.is_empty() {
            return buffer.len();
        }

        // Write the columns with proper padding for continuation lines
        let mut column_buffer = BytesMut::new();
        columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

        let column_output = String::from_utf8_lossy(&column_buffer);
        for line in column_output.lines() {
            if !line.trim().is_empty() {
                // Add padding
                for _ in 0..padding {
                    buffer.extend_from_slice(b" ");
                }
                buffer.extend_from_slice(COL_SEP.as_bytes());
                buffer.extend_from_slice(line.as_bytes());
                buffer.extend_from_slice(b"\n");
            }
        }

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = Vec::new();
        match &self.rest {
            Value::Object(map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_map_entry(key, value));
                });
            }
            v => {
                cells.push(colorize_json_value(&v));
            }
        }
        cells
    }
}

impl<'a> Display for Canonical<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer.utf8_chunks().try_for_each(|chunk| {
            let c = chunk.valid();
            write!(f, "{c}")
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use jiff::Timestamp;

    use super::*;
    use crate::config::ConfigOpts;
    use crate::logpatterns::{Printable, SourcedLine};

    #[test]
    fn layout_one() {
        // Try to set config, but don't fail if it's already set by another test
        let _ = config::set(ConfigOpts::default());

        let logline = r##"{
            "timestamp": "2025-08-01T10:45:03Z",
            "level": "CRITICAL",
            "message": "Database query failed",
            "query": "SELECT * FROM users WHERE id = ?",
            "error_code": "ER_NO_SUCH_TABLE",
            "elapsed": "250ms"
        }"##;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
        let stringy = parsed.to_string();
        let lines: Vec<&str> = stringy.split('\n').collect();
        let length = lines.len();

        assert_eq!(length, 4);
    }

    #[test]
    fn logline_deser() {
        let logline = r#"{
            "timestamp": "2025-07-30T17:41-07:00",
            "level":"INFO",
            "message": "I'm not crazy you're the ones who are crazy"}"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
        assert_eq!(parsed.message, "I'm not crazy you're the ones who are crazy");
        let logline = r#"{"timestamp": "2025-07-30T17:41-07:00",
            "message":"I'm not crazy you're the ones who are crazy",
            "level":"WARN",
            "request_id":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
        let reqid = parsed.request_id.expect("the request_id should exist");
        assert_eq!(reqid, "institutionalized");
        let logline = r#"{"ts": "2025-07-30T17:41-07:00",
            "msg":"I'm not crazy you're the ones who are crazy",
            "lvl":"CRITICAL",
            "requestId":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
        let reqid = parsed.request_id.expect("the request_id should exist");
        assert_eq!(reqid, "institutionalized");
    }

    #[test]
    fn not_loglines() {
        let logline = r#"Sometimes, I try to do things / And it just doesn't work out the way I want it to"#;
        let error = serde_json::from_str::<Message<'_>>(logline);
        assert!(error.is_err());
    }

    #[test]
    fn complex_logline() {
        let logline = r#"{
            "timestamp": "2025-07-03T20:37:35.098873Z",
            "level": "ERROR",
            "message": "HTTP PATCH /api/auth/login",
            "method": "PUT",
            "url": "/api/users",
            "status": 201,
            "elapsed": "1326ms",
            "size": 36159,
            "request_id": "req_641656",
            "remote_host": "10.0.191.79",
            "user_agent": "Python-requests/2.28.1",
            "user_id": 5491
        }"#;
        let parsed =
            serde_json::from_str::<Printable<'_>>(logline).expect("the HTTP patch message is a valid log line");
        let Printable::Canonical(canonical) = parsed else {
            panic!("we expected a canonical log line")
        };
        assert_eq!(canonical.message, "HTTP PATCH /api/auth/login");
    }

    #[test]
    fn log4j_stuff() {
        let logline = r#"{
            "timestamp":"2025-07-30T10:42:24.983Z",
            "message":"I like drinking coffee in the morning.",
            "level":"INFO",
            "threadName":"main",
            "requestid":"59F7A6B7-3EA3-4465-A4AE-CA3010F60695",
            "class":"com.sprockets.widgets.Dispatcher",
            "method":"dispatchWidget",
            "source":"Dispatcher.scala:11",
            "stackTrace":""
        }"#;
        let parsed =
            serde_json::from_str::<Printable<'_>>(logline).expect("the HTTP patch message is a valid log line");
        let Printable::Java(java) = parsed else {
            panic!("we expected a java log line")
        };
        assert_eq!(java.message, "I like drinking coffee in the morning.");
    }

    #[test]
    fn stable_sort_single_file_no_timestamps() {
        use std::path::PathBuf;

        // Lines from a single file with no timestamps - should maintain original order
        let lines = vec![
            (PathBuf::from("test.log"), 0, r#"{"message": "third line"}"#.to_string()),
            (PathBuf::from("test.log"), 1, r#"{"message": "first line"}"#.to_string()),
            (
                PathBuf::from("test.log"),
                2,
                r#"{"message": "second line"}"#.to_string(),
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

        // Should maintain original order since no timestamps and same file
        assert_eq!(sorted[0].line_number, 0);
        assert_eq!(sorted[1].line_number, 1);
        assert_eq!(sorted[2].line_number, 2);
    }

    #[test]
    fn test_stable_sort_single_file_with_same_timestamps() {
        use std::path::PathBuf;

        // Lines with identical timestamps - should maintain original order (stable
        // sort)
        let lines = vec![
            (
                PathBuf::from("test.log"),
                0,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "first"}"#.to_string(),
            ),
            (
                PathBuf::from("test.log"),
                1,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "second"}"#.to_string(),
            ),
            (
                PathBuf::from("test.log"),
                2,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "third"}"#.to_string(),
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

        // Should maintain original order due to stable sort with identical timestamps
        // first
        let Printable::TimeOnly(ref has_stamp) = sorted[0].parsed else {
            panic!("that really should have parsed as having a timestamp");
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);

        // second
        let Printable::TimeOnly(ref has_stamp) = sorted[1].parsed else {
            panic!("that really should have parsed as having a timestamp");
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);
        let obj = has_stamp.rest.as_object().expect("rest should be a json object");
        let message = obj.get("message").expect("there is a message in this bottle yeah-a");
        assert_eq!(message, "second");

        // third
        let Printable::TimeOnly(ref has_stamp) = sorted[2].parsed else {
            panic!("that really should have parsed as having a timestamp");
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);
        let obj = has_stamp.rest.as_object().expect("rest should be a json object");
        assert!(obj.contains_key("message"));
        let message = obj.get("message").expect("there is a message in this bottle yeah-a");
        assert_eq!(message, "third");
    }
}
