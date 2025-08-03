//! Structures and traits for log lines.

use std::borrow::Cow;
use std::fmt::Display;

use ansi_width::ansi_width;
use bytes::{BufMut, BytesMut};
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;
use term_grid::{Direction, Filling, GridOptions};
use textwrap::termwidth;

use crate::CONFIG;

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
    Message(Message<'a>),
    Json(GenericJson),
}

impl<'a> PrettyPrintable for Printable<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        match self {
            Printable::Message(message) => message.write(buffer),
            Printable::Json(generic) => generic.write(buffer),
        }
    }
}

impl<'a> Display for Printable<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
        let padding = if CONFIG.get().is_some_and(|v| v.show_time) {
            LEVEL_WIDTH + 1 + MODULE_WIDTH + ansi_width(COL_SEP)
        } else {
            LEVEL_WIDTH + ansi_width(COL_SEP)
        };

        let mut line = format!("{:>LEVEL_WIDTH$}", "json".bold().bright_blue());
        if show_time {
            line = format!("{line} {:<MODULE_WIDTH$}{COL_SEP}", " ");
        } else {
            line = format!("{line}{COL_SEP}");
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

        let grid = term_grid::Grid::new(
            cells,
            GridOptions {
                filling: Filling::Spaces(5),
                direction: Direction::LeftToRight,
                width: max_message_width,
            },
        );

        for chunk in grid.to_string().split('\n') {
            let trimmed = chunk.trim();
            if !trimmed.is_empty() {
                if !line.is_empty() {
                    line = format!("{line}{trimmed}");
                } else {
                    line = format!("{COL_SEP:>padding$}{trimmed}");
                }
                put_line(buffer, line.as_bytes());
                line.clear();
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

/// Layout / columnizing / formatting constants.
pub static LEVEL_WIDTH: usize = 8;
pub static MODULE_WIDTH: usize = 20;
pub static IDEAL_COL_WIDTH: usize = 40;
pub static COL_SEP: &str = " > ";

/// Write a finished line and end it with a newline.
fn put_line(buffer: &mut BytesMut, line: &[u8]) {
    buffer.put_slice(line);
    buffer.put_slice(&[0x0a; 1]);
}

impl<'a> PrettyPrintable for &Message<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_WIDTH - MODULE_WIDTH - 4; // col spacers count
        let min_message_width = std::cmp::min(max_message_width, IDEAL_COL_WIDTH);
        let padding = if CONFIG.get().is_some_and(|v| v.show_time) {
            LEVEL_WIDTH + 1 + MODULE_WIDTH + ansi_width(COL_SEP)
        } else {
            LEVEL_WIDTH + ansi_width(COL_SEP)
        };

        let mut line = format!("{:>LEVEL_WIDTH$}", self.level.bold().bright_blue());

        if show_time {
            if let Some(ref v) = self.timestamp {
                line = format!("{line} {:<MODULE_WIDTH$}{COL_SEP}", v.strftime("%F-%T").blue());
            } else if let Some(ref v) = self.request_id {
                line = format!("{line} {:<MODULE_WIDTH$}", v.bright_yellow());
            } else {
                line = format!("{line} {:<MODULE_WIDTH$}{COL_SEP}", " ");
            }
        } else {
            line = format!("{line}{COL_SEP}");
        }

        if self.message.contains('\n') {
            // This log message has newlines in it, like a stacktrace.
            // We're not going to rewrap it, but instead use the lines as-is.
            let mut chunks = self.message.split('\n');
            if let Some(next) = chunks.next() {
                line = format!("{line}{next}");
                put_line(buffer, line.as_bytes());
            }
            for chunk in chunks {
                line = format!("{COL_SEP:>padding$}{chunk}");
                put_line(buffer, line.as_bytes());
            }
        } else {
            let chunks = textwrap::wrap(&self.message, max_message_width);
            if chunks.len() == 1 {
                // we pad it out to the min
                line = format!("{line}{:<min_message_width$}", self.message.trim());
                put_line(buffer, line.as_bytes());
            } else {
                let mut chunk_iter = chunks.iter();
                if let Some(next) = chunk_iter.next() {
                    line = format!("{line}{}", next.trim());
                }
                for chunk in chunk_iter {
                    if ansi_width(chunk) + ansi_width(line.as_str()) < termwidth {
                        line = format!("{line} {chunk}");
                    } else {
                        put_line(buffer, line.as_bytes());
                        line = format!("{COL_SEP:>padding$}{chunk}");
                    }
                }
                put_line(buffer, line.as_bytes());
            }
        }

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

        // Now build our columns. I went for the lazy solution.
        let grid = term_grid::Grid::new(
            cells,
            GridOptions {
                filling: Filling::Spaces(5),
                direction: Direction::LeftToRight,
                width: max_message_width,
            },
        );
        for chunk in grid.to_string().split('\n') {
            let trimmed = chunk.trim();
            if !trimmed.is_empty() {
                line = format!("{COL_SEP:>padding$}{trimmed}");
                put_line(buffer, line.as_bytes());
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
}
