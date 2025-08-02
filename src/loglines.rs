//! Structures and traits for log lines.

use std::collections::HashMap;
use std::fmt::Display;

use serde::Deserialize;

use crate::colorize_json;
use crate::layout::{PrettyPrintable, layout};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Printable {
    Message(Message),
    Json(GenericJson),
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
    pub(crate) timestamp: Option<jiff::Timestamp>,
    /// The string message part of the log line.
    #[serde(alias = "msg")]
    pub(crate) message: String,
    /// Log level for this line.
    #[serde(alias = "lvl", alias = "severity")]
    pub(crate) level: String,
    /// The source module where the log line originated.
    #[serde(alias = "mod", alias = "lib")]
    pub(crate) module: Option<String>,
    /// The file where the log line originated.
    pub(crate) file: Option<String>,
    /// The line in the file where the log line originated.
    pub(crate) line: Option<usize>,
    /// A request id
    #[serde(alias = "requestId")]
    pub(crate) request_id: Option<String>,
    /// Remote host.
    #[serde(alias = "hostname", alias = "remote_host")]
    pub(crate) host: Option<String>,
    /// http method
    pub(crate) method: Option<String>,
    /// a url
    pub(crate) url: Option<String>,
    /// http response status code
    #[serde(alias = "statusCode", alias = "code")]
    pub(crate) status: Option<String>,
    /// elapsed time
    #[serde(alias = "elapsed_ms", alias = "elapsed_time")]
    pub(crate) elapsed: Option<String>,
    /// size of the written response
    #[serde(
        alias = "sent_bytes",
        alias = "length",
        alias = "written",
        alias = "bodylen",
        alias = "body",
        alias = "size"
    )]
    pub(crate) size: Option<String>,
    /// The unpredictable parts of the log line, which we'll handle more
    /// generically.
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
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
