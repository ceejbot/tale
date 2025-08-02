//! Structures and traits for log lines.

use std::collections::HashMap;
use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;

use crate::{CONFIG, layout};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Printable {
    Message(Message),
    Json(GenericJson),
}

pub trait PrettyPrintable
where
    Self: Display,
{
    fn cells(&self) -> Vec<String>;
}

impl PrettyPrintable for Printable {
    fn cells(&self) -> Vec<String> {
        match self {
            Printable::Message(message) => message.cells(),
            Printable::Json(generic) => generic.cells(),
        }
    }
}

impl Display for Printable {
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
    rest: HashMap<String, serde_json::Value>,
}

impl PrettyPrintable for &GenericJson {
    fn cells(&self) -> Vec<String> {
        self.rest.iter().map(|(key, value)| colorize_json(key, value)).collect()
    }
}

fn colorize_json(key: &str, value: &serde_json::Value) -> String {
    match value {
        Value::Null => format!("{}=null", key.dimmed()),
        Value::Bool(b) => format!("{}={}", key.dimmed(), b.cyan()),
        Value::Number(number) => format!("{}={}", key.dimmed(), number.bright_magenta()),
        Value::String(text) => format!("{}={}", key.dimmed(), text.green()),
        Value::Array(values) => format!(
            "{}: {}",
            key.dimmed(),
            serde_json::to_string_pretty(values).unwrap_or_default()
        ),
        Value::Object(map) => {
            if map.len() < 6 {
                let mut formatted = String::default();
                for (k, v) in map {
                    formatted = format!("{formatted}{} ", colorize_json(k, v));
                }
                formatted
            } else {
                format!("{}: object with {} keys", key.dimmed(), map.len().magenta())
            }
        }
    }
}

impl Display for GenericJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines = layout(self);
        lines.iter().try_for_each(|line| writeln!(f, "{line}"))
    }
}

/// A log line, somewhat flexibly formatted. There are two fields that we demand
/// be present: a log level and a message. We allow several possible names for
/// common log fields. The rest of the fields are mentioned only so we can print
/// them in a controlled order. We do not demand that they be present.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    timestamp: Option<jiff::Timestamp>,
    /// The string message part of the log line.
    #[serde(alias = "msg")]
    message: String,
    /// Log level for this line.
    #[serde(alias = "lvl", alias = "severity")]
    level: String,
    /// The source module where the log line originated.
    #[serde(alias = "mod", alias = "lib")]
    module: Option<String>,
    /// The file where the log line originated.
    file: Option<String>,
    /// The line in the file where the log line originated.
    line: Option<usize>,
    /// A request id
    #[serde(alias = "requestId")]
    request_id: Option<String>,
    /// Remote host.
    #[serde(alias = "hostname", alias = "remote_host")]
    host: Option<String>,
    /// http method
    method: Option<String>,
    /// a url
    url: Option<String>,
    /// http response status code
    #[serde(alias = "statusCode", alias = "code")]
    status: Option<String>,
    /// elapsed time
    #[serde(alias = "elapsed_ms", alias = "elapsed_time")]
    elapsed: Option<String>,
    /// size of the written response
    #[serde(
        alias = "sent_bytes",
        alias = "length",
        alias = "written",
        alias = "bodylen",
        alias = "body",
        alias = "size"
    )]
    size: Option<String>,
    /// The unpredictable parts of the log line, which we'll handle more
    /// generically.
    #[serde(flatten)]
    rest: serde_json::Value,
}

static LEVEL_COL: usize = 8;
static MODULE_COL: usize = 20;

impl PrettyPrintable for &Message {
    fn cells(&self) -> Vec<String> {
        let show_time = CONFIG.get().is_some_and(|v| v.show_time);
        let mut cells: Vec<String> = Vec::new();

        cells.push(format!("{:>LEVEL_COL$}", self.level.bold().bright_blue()));

        if show_time {
            if let Some(ref v) = self.timestamp {
                cells.push(format!("{:<MODULE_COL$}", v.strftime("%F-%T").blue()));
            } else if let Some(ref v) = self.request_id {
                cells.push(format!("{:<MODULE_COL$}", v.bright_yellow()));
            }
        }

        cells.push(format!("{}", self.message));

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
                    cells.push(colorize_json(key, value));
                });
            }
            _ => {
                cells.push(colorize_json("rest", &self.rest));
            }
        }
        cells
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines = layout(self);
        lines.iter().try_for_each(|line| writeln!(f, "{line}"))
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
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.message, "I'm not crazy you're the ones who are crazy");
        let logline = r#"{"timestamp": "2025-07-30T17:41-07:00",
            "message":"I'm not crazy you're the ones who are crazy",
            "level":"WARN",
            "request_id":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));
        let logline = r#"{"ts": "2025-07-30T17:41-07:00",
            "msg":"I'm not crazy you're the ones who are crazy",
            "lvl":"CRITICAL",
            "requestId":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));
    }

    #[test]
    fn not_loglines() {
        let logline = r#"Sometimes, I try to do things / And it just doesn't work out the way I want it to"#;
        let error = serde_json::from_str::<Message>(logline);
        assert!(error.is_err());
    }
}
