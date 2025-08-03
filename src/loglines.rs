//! Structures and traits for log lines.

use std::borrow::Cow;
use std::fmt::Display;

use ansi_width::ansi_width;
use bytes::BytesMut;
use humansize::{BINARY, format_size};
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;
use textwrap::termwidth;

use crate::{CONFIG, columns};

/// Layout / columnizing / formatting constants.
pub static LEVEL_WIDTH: usize = 8;
pub static MODULE_WIDTH: usize = 20;
pub static COL_SEP: &str = " > ";

/// Pre-compiled ANSI escape sequences for log levels (right-aligned to
/// LEVEL_WIDTH) Format: bright_blue + bold + right-aligned text + reset
static LEVEL_TRACE: &[u8] = b"\x1b[94m\x1b[1m   TRACE\x1b[0m\x1b[39m";
static LEVEL_DEBUG: &[u8] = b"\x1b[94m\x1b[1m   DEBUG\x1b[0m\x1b[39m";
static LEVEL_INFO: &[u8] = b"\x1b[94m\x1b[1m    INFO\x1b[0m\x1b[39m";
static LEVEL_WARN: &[u8] = b"\x1b[94m\x1b[1m    WARN\x1b[0m\x1b[39m";
static LEVEL_ERROR: &[u8] = b"\x1b[94m\x1b[1m   ERROR\x1b[0m\x1b[39m";
static LEVEL_FATAL: &[u8] = b"\x1b[94m\x1b[1m   FATAL\x1b[0m\x1b[39m";
static LEVEL_CRITICAL: &[u8] = b"\x1b[94m\x1b[1mCRITICAL\x1b[0m\x1b[39m";
static JSON_HEADER: &[u8] = b"\x1b[94m\x1b[1m    json\x1b[0m\x1b[39m";

/// Pre-compiled ANSI escape sequences for timestamp formatting
static TIMESTAMP_START: &[u8] = b"\x1b[94m"; // blue color
static TIMESTAMP_END: &[u8] = b"\x1b[39m"; // reset to default color

/// Get pre-compiled ANSI bytes for a log level, with fallback formatting
fn get_level_bytes(level: &str) -> &'static [u8] {
    match level.to_uppercase().as_str() {
        "TRACE" => LEVEL_TRACE,
        "DEBUG" => LEVEL_DEBUG,
        "INFO" => LEVEL_INFO,
        "WARN" | "WARNING" => LEVEL_WARN,
        "ERROR" => LEVEL_ERROR,
        "FATAL" => LEVEL_FATAL,
        "CRITICAL" | "CRIT" => LEVEL_CRITICAL,
        _ => LEVEL_INFO, // Default fallback
    }
}

/// Write a formatted timestamp directly to buffer with blue coloring and
/// left-padding Format: blue timestamp + reset + padding to MODULE_WIDTH +
/// COL_SEP
fn write_timestamp_column(buffer: &mut BytesMut, timestamp: &jiff::Timestamp) {
    let timestamp_str = timestamp.strftime("%F-%T").to_string();

    // Write: blue_start + timestamp + blue_end + padding + separator
    buffer.extend_from_slice(TIMESTAMP_START);
    buffer.extend_from_slice(timestamp_str.as_bytes());
    buffer.extend_from_slice(TIMESTAMP_END);

    // Calculate padding needed to reach MODULE_WIDTH
    let timestamp_len = timestamp_str.len(); // 19 chars for "%F-%T" format
    if timestamp_len < MODULE_WIDTH {
        let padding_needed = MODULE_WIDTH - timestamp_len;
        for _ in 0..padding_needed {
            buffer.extend_from_slice(b" ");
        }
    }

    buffer.extend_from_slice(COL_SEP.as_bytes());
}

pub fn colorize_json_value(value: &serde_json::Value) -> String {
    match value {
        Value::Null => "null".red().to_string(),
        Value::Bool(b) => b.cyan().to_string(),
        Value::Number(number) => number.bright_magenta().to_string(),
        Value::String(text) => {
            let val = format!("\"{text}\"");
            val.green().to_string()
        }
        Value::Array(values) => {
            let valstrs: Vec<String> = values.iter().map(colorize_json_value).collect();
            valstrs.join(", ")
        }
        Value::Object(object) => {
            format!(
                "{}{}{}",
                "{ ",
                object
                    .iter()
                    .map(|(k, v)| colorize_map_entry(k, v))
                    .collect::<Vec<String>>()
                    .join(", "),
                " }"
            )
        }
    }
}

pub fn colorize_map_entry(key: &str, value: &serde_json::Value) -> String {
    match value {
        Value::Null => {
            format!("{}={}", key.dimmed(), "null".red())
        }
        Value::Bool(b) => format!("{}={}", key.dimmed(), b.cyan()),
        Value::Number(number) => format!("{}={}", key.dimmed(), number.bright_magenta()),
        Value::String(text) => {
            let val = format!("\"{text}\"");
            format!("{}={}", key.dimmed(), val.green())
        }
        Value::Array(values) => {
            let valstrs: Vec<String> = values.iter().map(colorize_json_value).collect();
            valstrs.join(", ")
        }
        Value::Object(_) => {
            format!("{}={}", key.dimmed(), colorize_json_value(value))
        }
    }
}

/// All log messages get formatted the same way. Extracted out of
/// the original message formatter.
fn format_message(message: &str, buffer: &mut BytesMut, padding: usize, max_message_width: usize) {
    if message.contains('\n') {
        // This log message has newlines in it, like a stacktrace.
        // We're not going to rewrap it, but instead use the lines as-is.
        let mut chunks = message.split('\n');
        if let Some(next) = chunks.next() {
            buffer.extend_from_slice(next.as_bytes());
            buffer.extend_from_slice(b"\n");
        }
        for chunk in chunks {
            // Add padding for continuation lines
            for _ in 0..padding {
                buffer.extend_from_slice(b" ");
            }
            buffer.extend_from_slice(COL_SEP.as_bytes());
            buffer.extend_from_slice(chunk.as_bytes());
            buffer.extend_from_slice(b"\n");
        }
    } else {
        // a rough and ready check that probably works
        if message.len() <= max_message_width {
            buffer.extend_from_slice(message.as_bytes());
            buffer.extend_from_slice(b"\n");
        } else {
            let chunks = textwrap::wrap(message, max_message_width);
            let mut first = true;
            for chunk in chunks {
                if first {
                    buffer.extend_from_slice(chunk.trim().as_bytes());
                    buffer.extend_from_slice(b"\n");
                    first = false;
                } else {
                    // Add padding for continuation lines
                    for _ in 0..padding {
                        buffer.extend_from_slice(b" ");
                    }
                    buffer.extend_from_slice(COL_SEP.as_bytes());
                    buffer.extend_from_slice(chunk.trim().as_bytes());
                    buffer.extend_from_slice(b"\n");
                }
            }
        }
    }
}

/// If you can be pretty-printed, you write into a mutable byte buffer.
/// We don't have any opinions about what you write; pretty is in the eye
/// of the implementor.
pub trait PrettyPrintable
where
    Self: Display,
{
    fn write(&self, buffer: &mut BytesMut) -> usize;
}

/// An enum to help serde deserialize incoming log lines. There are some
/// we decide are log lines with fields we recognize, and some that are just
/// json we pretty-print. And then there's plain text.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, bound(deserialize = "'de: 'a"))]
pub enum Printable<'a> {
    Canonical(Box<Canonical<'a>>),
    Message(Box<Message<'a>>),
    Json(GenericJson),
}

impl<'a> PrettyPrintable for Printable<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        match self {
            Printable::Canonical(canonical) => canonical.as_ref().write(buffer),
            Printable::Message(message) => message.as_ref().write(buffer),
            Printable::Json(generic) => generic.write(buffer),
        }
    }
}

impl<'a> Display for Printable<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Printable::Canonical(canonical) => canonical.fmt(f),
            Printable::Message(message) => message.fmt(f),
            Printable::Json(generic) => generic.fmt(f),
        }
    }
}

/// We couldn't parse this json as a log line, even minimally, but it's
/// json so we'll print it.
#[derive(Debug, Clone, Deserialize)]
pub struct GenericJson {
    #[serde(flatten)]
    rest: serde_json::Value,
}

impl PrettyPrintable for &GenericJson {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
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

        // Write the columns, handling multi-line output with proper padding
        if !cells.is_empty() {
            let mut column_buffer = BytesMut::new();
            columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

            let column_output = String::from_utf8_lossy(&column_buffer);
            let mut first_line = true;

            for line in column_output.lines() {
                if first_line {
                    buffer.extend_from_slice(line.as_bytes());
                    first_line = false;
                } else {
                    // Add padding for continuation lines
                    for _ in 0..padding {
                        buffer.extend_from_slice(b" ");
                    }
                    buffer.extend_from_slice(COL_SEP.as_bytes());
                    buffer.extend_from_slice(line.as_bytes());
                }
                buffer.extend_from_slice(b"\n");
            }
        }

        buffer.len()
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

/// This is a possibly familiar format that we require conformance to.
#[derive(Debug, Clone, Deserialize)]
pub struct Canonical<'a> {
    timestamp: jiff::Timestamp,
    level: Cow<'a, str>,
    message: Cow<'a, str>,
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
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
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
        let mut formatted = format!("{} {} {}", self.method.blue(), self.url.blue(), self.status.blue());
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);
        formatted.clear();

        let sized = format_size(self.size, BINARY);
        formatted = format!("  {}{}", "size=".dimmed(), sized.bright_magenta());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        // tedious repetition but
        formatted = format!("  {}{}", "elapsed=".dimmed(), self.elapsed.magenta());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        formatted = format!("  {}{}", "remote_host=".dimmed(), self.remote_host.blue());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
            count = padding + 3
        }
        buffer.extend_from_slice(formatted.as_bytes());
        count += ansi_width(&formatted);

        formatted = format!("  {}{}", "user_agent=".dimmed(), self.user_agent.green());
        if count + ansi_width(&formatted) >= max_message_width {
            start_new_line(buffer, padding);
        }
        buffer.extend_from_slice(formatted.as_bytes());

        let mut cells = Vec::new();
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
}

fn start_new_line(buffer: &mut BytesMut, padding: usize) {
    buffer.extend_from_slice(b"\n");
    for _ in 0..padding {
        buffer.extend_from_slice(b" ");
    }
    buffer.extend_from_slice(COL_SEP.as_bytes());
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

/*

CRITICAL module::thing::foobl > message here with rest of width
LEVEL    MODULE or TIME       > message line wrapped if no line endings in it; as-is if \n present
8char    20 char              > termwidth - 32 char wide
blank    timing / bytes?      > STATUS VERB URL if present
         bytes                > key: value pairs to fill space, wrapping, left aligned at this column

 */

impl<'a> PrettyPrintable for &Message<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
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
                // Add space + formatted request_id (no separator yet)
                buffer.extend_from_slice(b" ");
                let formatted = format!("{:<MODULE_WIDTH$}", v.bright_yellow());
                buffer.extend_from_slice(formatted.as_bytes());
            } else {
                // Add space + padding + separator
                buffer.extend_from_slice(b" ");
                let formatted = format!("{:<MODULE_WIDTH$}{COL_SEP}", " ");
                buffer.extend_from_slice(formatted.as_bytes());
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
