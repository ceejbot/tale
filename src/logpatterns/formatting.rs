//! Formatting and colorizing log lines.

/*

CRITICAL module::thing::foobl > message here with rest of width
LEVEL    MODULE or TIME       > message line wrapped if no line endings in it; as-is if \n present
8char    20 char              > termwidth - 32 char wide
blank    timing / bytes?      > STATUS VERB URL if present
         bytes                > key: value pairs to fill space, wrapping, left aligned at this column

 */

use bytes::BytesMut;
use owo_colors::OwoColorize;
use serde_json::Value;

/// Layout / columnizing / formatting constants.
pub static LEVEL_WIDTH: usize = 8;
pub static MODULE_WIDTH: usize = 20;
pub static COL_SEP: &str = " > ";

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

/// Write a formatted timestamp directly to buffer with blue coloring and
/// left-padding Format: blue timestamp + reset + padding to MODULE_WIDTH +
/// COL_SEP
pub fn write_timestamp_column(buffer: &mut BytesMut, timestamp: &jiff::Timestamp) {
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

/// Write a formatted request_id directly to buffer with bright_yellow coloring
/// and left-padding Format: bright_yellow + request_id + reset + padding to
/// MODULE_WIDTH + COL_SEP
pub fn write_request_id_column(buffer: &mut BytesMut, request_id: &str) {
    // Write: bright_yellow_start + request_id + reset + padding + separator
    buffer.extend_from_slice(REQUEST_ID_START);
    buffer.extend_from_slice(request_id.as_bytes());
    buffer.extend_from_slice(REQUEST_ID_END);

    // Calculate padding needed to reach MODULE_WIDTH
    let request_id_len = request_id.len();
    if request_id_len < MODULE_WIDTH {
        let padding_needed = MODULE_WIDTH - request_id_len;
        for _ in 0..padding_needed {
            buffer.extend_from_slice(b" ");
        }
    }

    buffer.extend_from_slice(COL_SEP.as_bytes());
}

/// Write empty module column with just padding and separator
pub fn write_empty_module_column(buffer: &mut BytesMut) {
    for _ in 0..MODULE_WIDTH {
        buffer.extend_from_slice(b" ");
    }
    buffer.extend_from_slice(COL_SEP.as_bytes());
}

pub fn start_new_line(buffer: &mut BytesMut, padding: usize) {
    buffer.extend_from_slice(b"\n");
    for _ in 0..padding {
        buffer.extend_from_slice(b" ");
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

pub fn write_rest<T>(line: T, buffer: &mut BytesMut, max_message_width: usize, padding: usize) -> usize
where
    T: super::PrettyPrintable,
{
    let cells = line.cells();

    // Write the columns, handling multi-line output with proper padding
    if !cells.is_empty() {
        let mut column_buffer = BytesMut::new();
        super::columns::write_columns(&mut column_buffer, &cells, max_message_width, 5);

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
