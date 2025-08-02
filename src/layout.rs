use std::cmp::max;
use std::fmt::Display;

use ansi_width::ansi_width;
use bytes::BufMut;
use bytes::BytesMut;
use owo_colors::OwoColorize;
use serde_json::Value;
use textwrap::termwidth;

use crate::CONFIG;
use crate::Message;

pub fn colorize_json(key: &str, value: &serde_json::Value, width: usize) -> String {
    let remaining = max(width - ansi_width(key), 10);
    match value {
        Value::Null => {
            format!("{}{:<remaining$}", key.dimmed(), "=null")
        }
        Value::Bool(b) => format!("{}={:<remaining$}", key.dimmed(), b.cyan()),
        Value::Number(number) => format!("{}={:<remaining$}", key.dimmed(), number.bright_magenta()),
        Value::String(text) => {
            let val = format!("\"{text}\"");
            format!("{}={:<remaining$}", key.dimmed(), val.green())
        }
        Value::Array(values) => format!(
            "{}={:<remaining$}",
            key.dimmed(),
            serde_json::to_string_pretty(values).unwrap_or_default()
        ),
        Value::Object(map) => {
            if map.len() < 6 {
                let mut formatted = "{ ".to_string();
                for (k, v) in map {
                    formatted = format!("{formatted}{:<} ", colorize_json(k, v, remaining));
                }
                format!("{formatted}}}")
            } else {
                format!("{}: object with {} keys", key.dimmed(), map.len().magenta())
            }
        }
    }
}

pub trait PrettyPrintable
where
    Self: Display,
{
    fn cells(&self) -> Vec<String>;
    fn write(&self, buffer: &mut BytesMut) -> usize;
}

/*

CRITICAL module::thing::foobl > message here with rest of width
LEVEL    MODULE or TIME       > message line wrapped if no line endings in it; as-is if \n present
8char    20 char              > termwidth - 32 char wide
blank    timing / bytes       > STATUS VERB URL if present
         bytes                > key: value pairs to fill space, wrapping, left aligned at this column

 */
pub static LEVEL_COL: usize = 8;
pub static MODULE_COL: usize = 20;
pub static COL_SEP: &str = " > ";

pub fn layout<T>(logline: T) -> Vec<String>
where
    T: PrettyPrintable,
{
    let termwidth = termwidth();
    let max_message_width = termwidth - LEVEL_COL - MODULE_COL - 4; // col spacers count
    let min_message_width = std::cmp::min(max_message_width, 50);
    let (num_cols, padding) = if CONFIG.get().is_some_and(|v| v.show_time) {
        (3, LEVEL_COL + 1 + MODULE_COL + ansi_width(COL_SEP))
    } else {
        (2, LEVEL_COL + ansi_width(COL_SEP))
    };

    let cells = logline.cells();
    if cells.is_empty() {
        return Vec::new();
    };

    // we now lay out the cells in a new vector of lines, re-using a
    // pre-alloced buffer to stay thrifty. (TODO) They are in the correct order
    // already, so all we have to do is fill in our columns.
    let mut lines = Vec::new();
    let mut cell_iter = cells.into_iter();

    let mut line: String;
    // first line is special
    if num_cols == 3 {
        let first = cell_iter.next().unwrap_or_default();
        let second = cell_iter.next().unwrap_or_default();
        line = format!("{first} {second}{COL_SEP}");
    } else {
        let first = cell_iter.next().unwrap_or_default();
        line = format!("{first}{COL_SEP}");
    }

    // Now the fun starts. We want to fill the next column with a long message if
    // needed.
    let message = cell_iter.next().unwrap_or_default();
    if message.contains('\n') {
        // We're not going to rewrap it, but instead use the lines as-is.
        let mut chunks = message.split('\n');
        if let Some(next) = chunks.next() {
            lines.push(format!("{line}{next}"));
            line = format!("{COL_SEP:>padding$}");
        }
        for chunk in chunks {
            lines.push(format!("{COL_SEP:>padding$}{chunk}"));
        }
    } else {
        let chunks = textwrap::wrap(message.as_str(), max_message_width);
        if chunks.len() == 1 {
            // we pad it out to the min
            line = format!("{line}{message:<min_message_width$}");
        } else {
            let mut chunk_iter = chunks.iter();
            if let Some(next) = chunk_iter.next() {
                lines.push(format!("{line}{next}"));
                // the next chunk goes on a new line
                line = format!("{COL_SEP:>padding$}");
            }
            for chunk in chunk_iter {
                lines.push(format!("{COL_SEP:>padding$}{chunk}"));
            }
        }
    }

    for next in cell_iter {
        if ansi_width(line.as_str()) + ansi_width(next.as_str()) < termwidth {
            line = format!("{line}{next}  ");
        } else {
            lines.push(line);
            line = format!("{COL_SEP:>padding$}{next}");
        }
    }
    lines.push(line);

    lines
}

impl PrettyPrintable for &Message {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
        let termwidth = termwidth();
        let max_message_width = termwidth - LEVEL_COL - MODULE_COL - 4; // col spacers count
        let min_message_width = std::cmp::min(max_message_width, 50);
        let padding = if CONFIG.get().is_some_and(|v| v.show_time) {
            LEVEL_COL + 1 + MODULE_COL + ansi_width(COL_SEP)
        } else {
            LEVEL_COL + ansi_width(COL_SEP)
        };

        let mut line = format!("{:>LEVEL_COL$}", self.level.bold().bright_blue());

        if show_time {
            if let Some(ref v) = self.timestamp {
                line = format!("{line} {:<MODULE_COL$}{COL_SEP}", v.strftime("%F-%T").blue());
            } else if let Some(ref v) = self.request_id {
                line = format!("{line} {:<MODULE_COL$}", v.bright_yellow());
            } else {
                line = format!("{line} {:<MODULE_COL$}{COL_SEP}", " ");
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
                buffer.put(line.as_bytes());
            }
            for chunk in chunks {
                line = format!("{COL_SEP:>padding$}{chunk}");
                buffer.put(line.as_bytes());
            }
        } else {
            let chunks = textwrap::wrap(self.message.as_str(), max_message_width);
            if chunks.len() == 1 {
                // we pad it out to the min
                line = format!("{line}{:<min_message_width$}", self.message);
                buffer.put(line.as_bytes());
            } else {
                let mut chunk_iter = chunks.iter();
                if let Some(next) = chunk_iter.next() {
                    buffer.put(format!("{line}{next}").as_bytes());
                }
                for chunk in chunk_iter {
                    buffer.put(format!("{COL_SEP:>padding$}{chunk}").as_bytes());
                }
            }
        }

        // Now we walk through all the other fields, treating some specially,
        // building our list of cells somewhat inefficiently. We pad everything
        // out to the ideal column width as we go, to avoid having to work to
        // avoid counting ansi escapes.
        let ideal_width = 50;
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
        cells.push(format!("{logloc:<ideal_width$}").yellow().to_string());

        // if we didn't already snag the request id
        if !show_time && let Some(ref v) = self.request_id {
            let width = ideal_width - ansi_width(v);
            let padded = format!("{v:<width$}");
            cells.push(format!("{}{}", "reqid=".dimmed(), padded.bright_yellow()));
        }

        if let Some(ref v) = self.status {
            // TODO transform this into `200 OK` or similar canonical form
            let width = ideal_width - ansi_width(v);
            let padded = format!("{v:<width$}");
            cells.push(format!("{} {}", "status".dimmed(), padded.purple()));
        }

        // Units and format are unpredictable, so we leave it as-is.
        if let Some(ref v) = self.elapsed {
            let width = ideal_width - ansi_width(v);
            let padded = format!("{v:<width$}");
            cells.push(format!("{}{}", "elapsed=".dimmed(), padded.bright_purple()));
        }

        // Same as above.
        if let Some(ref v) = self.size {
            let width = ideal_width - ansi_width(v);
            let padded = format!("{v:<width$}");
            cells.push(format!("{}{}", "size=".dimmed(), padded.bright_purple()));
        }
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_json(key, value, ideal_width));
                });
            }
            _ => {
                cells.push(colorize_json("rest", &self.rest, ideal_width));
            }
        }

        // Now build our columns
        for next in cells.iter() {
            // pad using the real width
            let real = (padding as i64 - ansi_width(next) as i64).unsigned_abs() as usize;
            if ansi_width(line.as_str()) + ansi_width(next.as_str()) < termwidth {
                line = format!("{line}{next}{:real$}  ", " ");
            } else {
                buffer.put(line.as_bytes());
                line = format!("{COL_SEP:>padding$}{next}{:real$}", " ");
            }
        }

        // if it is this easy I can get rid of the return value
        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
        let mut cells: Vec<String> = Vec::new();

        cells.push(format!("{:>LEVEL_COL$}", self.level.bold().bright_blue()));

        if show_time {
            if let Some(ref v) = self.timestamp {
                cells.push(format!("{:<MODULE_COL$}", v.strftime("%F-%T").blue()));
            } else if let Some(ref v) = self.request_id {
                cells.push(format!("{:<MODULE_COL$}", v.bright_yellow()));
            } else {
                cells.push(format!("{:<MODULE_COL$}", " "));
            }
        }

        cells.push(self.message.to_string());

        // if we didn't already snag the request id
        if !show_time && let Some(ref v) = self.request_id {
            cells.push(format!("{}{}", "reqid=".dimmed(), v.bright_yellow()));
        }

        // Work a little bit to show the log line location coherently.
        // Probably if one is present, all are present, but not always.
        if let Some(ref module) = self.module
            && let Some(ref fname) = self.file
            && let Some(lineno) = self.line
        {
            cells.push(format!("{module}: {fname}:{lineno}").yellow().to_string());
        } else if let Some(ref fname) = self.file
            && let Some(lineno) = self.line
        {
            cells.push(format!("{fname}:{lineno}").yellow().to_string());
        } else if let Some(ref fname) = self.file {
            cells.push(format!("{}", fname.yellow()));
        }

        if let Some(ref v) = self.host {
            cells.push(format!("{}{}", "host=".dimmed(), v.green()));
        }

        // If we are logging an http request, we might have these.
        // We glue them together if we have them all.
        if let Some(ref method) = self.method
            && let Some(ref url) = self.url
            && let Some(ref status) = self.status
        {
            cells.push(format!("{method} {url} {status}").blue().to_string());
        } else if let Some(ref method) = self.method
            && let Some(ref url) = self.url
        {
            cells.push(format!("{method} {url}").blue().to_string());
        } else if let Some(ref url) = self.url {
            cells.push(format!("{}", url.blue()));
        }

        if let Some(ref v) = self.status {
            cells.push(format!("{}", v.purple()));
        }

        // Units and format are unpredictable, so we leave it as-is.
        if let Some(ref v) = self.elapsed {
            cells.push(format!("{}{}", "elapsed=".dimmed(), v.bright_purple()));
        }

        // We can't improve this because units & format are unknown.
        if let Some(ref v) = self.size {
            cells.push(format!("{}{}", "size=".dimmed(), v.bright_purple()));
        }
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(colorize_json(key, value, MODULE_COL * 2));
                });
            }
            _ => {
                cells.push(colorize_json("rest", &self.rest, MODULE_COL * 2));
            }
        }
        cells
    }
}
