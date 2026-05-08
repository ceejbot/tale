//! Formatting and colorizing log lines.

/*

CRITICAL module::thing::foobl │ message here with rest of width
LEVEL    MODULE or TIME       │ message line wrapped if no line endings in it; as-is if \n present
8char    20 char              │ termwidth - 32 char wide
blank    timing / bytes?      │ STATUS VERB URL if present
         bytes                │ key: value pairs to fill space, wrapping, left aligned at this column

 */

use bytes::BytesMut;
use owo_colors::OwoColorize;
use serde_json::Value;
use textwrap::termwidth;

use crate::config;

/// Layout / column / formatting constants.
pub static LEVEL_WIDTH: usize = 8;
pub static MODULE_WIDTH: usize = 20;
pub static COL_SEP: &str = " │ ";

/// Pre-compiled ANSI escape sequences for log levels (right-aligned to
/// LEVEL_WIDTH) Format: bright_blue + bold + right-aligned text + reset
pub static LEVEL_TRACE: &[u8] = b"\x1b[94m\x1b[1m   TRACE\x1b[0m\x1b[39m";
pub static LEVEL_DEBUG: &[u8] = b"\x1b[94m\x1b[1m   DEBUG\x1b[0m\x1b[39m";
pub static LEVEL_INFO: &[u8] = b"\x1b[94m\x1b[1m    INFO\x1b[0m\x1b[39m";
pub static LEVEL_WARN: &[u8] = b"\x1b[94m\x1b[1m    WARN\x1b[0m\x1b[39m";
pub static LEVEL_ERROR: &[u8] = b"\x1b[94m\x1b[1m   ERROR\x1b[0m\x1b[39m";
pub static LEVEL_FATAL: &[u8] = b"\x1b[94m\x1b[1m   FATAL\x1b[0m\x1b[39m";
pub static LEVEL_CRITICAL: &[u8] = b"\x1b[94m\x1b[1mCRITICAL\x1b[0m\x1b[39m";
pub static JSON_HEADER: &[u8] = b"\x1b[94m\x1b[1m    json\x1b[0m\x1b[39m";

/// Pre-compiled ANSI escape sequences for timestamp formatting
pub static TIMESTAMP_START: &[u8] = b"\x1b[94m"; // blue color
pub static TIMESTAMP_END: &[u8] = b"\x1b[39m"; // reset to default color

/// Pre-compiled ANSI escape sequences for request_id formatting
pub static REQUEST_ID_START: &[u8] = b"\x1b[93m\x1b[1m"; // bright_yellow
pub static REQUEST_ID_END: &[u8] = b"\x1b[0m\x1b[39m"; // reset

/// Get pre-compiled ANSI bytes for a log level, with fallback formatting
pub fn get_level_bytes(level: &str) -> &'static [u8] {
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

/// Append `count` spaces to `buffer`. A small helper so call sites don't
/// repeat the `buffer.resize(buffer.len() + count, b' ')` idiom.
pub fn pad_spaces(buffer: &mut BytesMut, count: usize) {
    buffer.resize(buffer.len() + count, b' ');
}

/// Write a formatted timestamp directly to buffer with blue coloring and
/// left-padding. Format: blue timestamp + reset + padding to MODULE_WIDTH +
/// COL_SEP.
pub fn write_timestamp_column(buffer: &mut BytesMut, timestamp: &jiff::Timestamp) {
    let timestamp_str = timestamp.strftime("%F-%T").to_string();

    buffer.extend_from_slice(TIMESTAMP_START);
    buffer.extend_from_slice(timestamp_str.as_bytes());
    buffer.extend_from_slice(TIMESTAMP_END);

    pad_spaces(buffer, MODULE_WIDTH.saturating_sub(timestamp_str.len()));
    buffer.extend_from_slice(COL_SEP.as_bytes());
}

/// Write a formatted request_id directly to buffer with bright_yellow coloring
/// and left-padding. Format: bright_yellow + request_id + reset + padding to
/// MODULE_WIDTH + COL_SEP.
pub fn write_request_id_column(buffer: &mut BytesMut, request_id: &str) {
    buffer.extend_from_slice(REQUEST_ID_START);
    buffer.extend_from_slice(request_id.as_bytes());
    buffer.extend_from_slice(REQUEST_ID_END);

    pad_spaces(buffer, MODULE_WIDTH.saturating_sub(request_id.len()));
    buffer.extend_from_slice(COL_SEP.as_bytes());
}

/// Write empty module column with just padding and separator
pub fn write_empty_module_column(buffer: &mut BytesMut) {
    pad_spaces(buffer, MODULE_WIDTH);
    buffer.extend_from_slice(COL_SEP.as_bytes());
}

/// Layout dimensions for log line rendering, derived from terminal width and
/// the user's `--timestamps` preference. Compute once per line via `current()`.
pub struct LayoutMetrics {
    /// Width available for the message portion (after level + module/time
    /// columns + separators).
    pub max_message_width: usize,
    /// Number of leading spaces on continuation lines (so they line up under
    /// the message column).
    pub padding: usize,
    /// Whether the user wants the timestamp column shown.
    pub show_time: bool,
}

impl LayoutMetrics {
    /// Compute layout from current global config and terminal width.
    pub fn current() -> Self {
        let show_time = config::show_time();
        // Layout: LEVEL_WIDTH (8) + " " (1) + MODULE_WIDTH (20) + COL_SEP (3) + message
        // The literal `4` collapses the space-and-separator characters.
        let max_message_width = termwidth().saturating_sub(LEVEL_WIDTH + MODULE_WIDTH + 4);
        let padding = if show_time {
            LEVEL_WIDTH + 1 + MODULE_WIDTH
        } else {
            LEVEL_WIDTH
        };
        Self {
            max_message_width,
            padding,
            show_time,
        }
    }
}

/// Render `cells` as elastic-tabstop columns and append the resulting lines to
/// `buffer`, each preceded by `metrics.padding` spaces and a `COL_SEP`. Used by
/// the variants that write a header line *first* and then the cells on
/// subsequent lines (Message, Java, Canonical, Logfmt).
pub fn write_cells_with_padding(buffer: &mut BytesMut, cells: &[String], metrics: &LayoutMetrics) {
    if cells.is_empty() {
        return;
    }

    let mut column_buffer = BytesMut::new();
    super::columns::write_columns(&mut column_buffer, cells, metrics.max_message_width, 5);

    let column_output = String::from_utf8_lossy(&column_buffer);
    for line in column_output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        pad_spaces(buffer, metrics.padding);
        buffer.extend_from_slice(COL_SEP.as_bytes());
        buffer.extend_from_slice(line.as_bytes());
        buffer.extend_from_slice(b"\n");
    }
}

/// Render a JSON scalar (or composite) with our color scheme:
/// keys=dimmed, strings=green (quoted), numbers=bright_yellow, booleans=cyan,
/// null=red. Arrays render their elements comma-joined; objects render as
/// `{ k=v, … }`.
pub fn colorize_json_value(value: &serde_json::Value) -> String {
    match value {
        Value::Null => "null".red().to_string(),
        Value::Bool(b) => b.cyan().to_string(),
        Value::Number(number) => number.bright_yellow().to_string(),
        Value::String(text) => format!("\"{text}\"").green().to_string(),
        Value::Array(values) => values.iter().map(colorize_json_value).collect::<Vec<_>>().join(", "),
        Value::Object(object) => {
            let inner = object
                .iter()
                .map(|(k, v)| colorize_map_entry(k, v))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {inner} }}")
        }
    }
}

/// Render a key=value pair using the same color scheme as `colorize_json_value`.
/// Arrays are rendered without the `key=` prefix (just the comma-joined values),
/// matching the original semantics — arrays are typically lists of structured
/// data where the key context is already provided by surrounding columns.
pub fn colorize_map_entry(key: &str, value: &serde_json::Value) -> String {
    if let Value::Array(values) = value {
        return values.iter().map(colorize_json_value).collect::<Vec<_>>().join(", ");
    }
    format!("{}={}", key.dimmed(), colorize_json_value(value))
}

/// Write a continuation line: `padding` spaces, the column separator, and
/// `content` followed by a newline. Used by both message-wrapping and
/// stacktrace-line code paths.
fn write_continuation(buffer: &mut BytesMut, content: &[u8], padding: usize) {
    pad_spaces(buffer, padding);
    buffer.extend_from_slice(COL_SEP.as_bytes());
    buffer.extend_from_slice(content);
    buffer.extend_from_slice(b"\n");
}

/// All log messages get formatted the same way. Extracted out of
/// the original message formatter.
pub fn format_message(message: &str, buffer: &mut BytesMut, padding: usize, max_message_width: usize) {
    if message.contains('\n') {
        // This log message has newlines in it, like a stacktrace.
        // We're not going to rewrap it, but instead use the lines as-is.
        let mut chunks = message.split('\n');
        if let Some(next) = chunks.next() {
            buffer.extend_from_slice(next.as_bytes());
            buffer.extend_from_slice(b"\n");
        }
        for chunk in chunks {
            write_continuation(buffer, chunk.as_bytes(), padding);
        }
    } else if message.len() <= max_message_width {
        // a rough and ready check that probably works
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
                write_continuation(buffer, chunk.trim().as_bytes(), padding);
            }
        }
    }
}

/// Render `cells` as elastic-tabstop columns. The first column-line continues
/// from whatever the caller already wrote (no leading padding); subsequent
/// lines get `padding` spaces and a `COL_SEP`. Used by Timestamped and
/// GenericJson, which write a header column and then continue with cells on
/// the same line.
pub fn write_rest<T>(line: T, buffer: &mut BytesMut, max_message_width: usize, padding: usize) -> usize
where
    T: super::PrettyPrintable,
{
    let cells = line.cells();
    if cells.is_empty() {
        return buffer.len();
    }

    let mut column_buffer = BytesMut::new();
    super::columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

    let column_output = String::from_utf8_lossy(&column_buffer);
    let mut first_line = true;
    for line in column_output.lines() {
        if first_line {
            buffer.extend_from_slice(line.as_bytes());
            first_line = false;
        } else {
            pad_spaces(buffer, padding);
            buffer.extend_from_slice(COL_SEP.as_bytes());
            buffer.extend_from_slice(line.as_bytes());
        }
        buffer.extend_from_slice(b"\n");
    }

    buffer.len()
}
