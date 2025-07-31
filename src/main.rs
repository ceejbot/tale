//! Pretty-print newline-delimited json (ndjson) logs.
//! No more, no less.

use std::fmt::Display;
use std::io::{self, BufRead, Write};

use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;

/// A log line, somewhat flexibly formatted. There are only two fields that we
/// demand be present: a timestamp and a message. We allow several possible names
/// for these fields. The rest of the fields are mentioned only so we can print
/// them in a controlled order. We do not demand that they be present.
#[derive(Debug, Clone, Deserialize)]
struct Message {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    timestamp: jiff::Timestamp,
    /// The string message part of the log line.
    #[serde(alias = "msg")]
    message: String,
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
    #[serde(alias = "sent_bytes", alias = "length", alias = "written")]
    bytes: Option<String>,
    /// The unpredictable parts of the log line we want to handle generally.
    #[serde(flatten)]
    rest: serde_json::Value,
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut spacing = "           ";
        write!(
            f,
            "{} {}\n",
            self.timestamp.strftime("%F %T %:z").yellow(),
            self.message.bold()
        )?;

        if let Some(ref v) = self.request_id {
            write!(f, "{}{}", spacing, v.bright_blue())?;
            spacing = " ";
        }
        if let Some(ref v) = self.host {
            write!(f, "{}{}", spacing, v.purple())?;
            spacing = " ";
        }
        if let Some(ref v) = self.method {
            write!(f, "{}{}", spacing, v.blue())?;
            spacing = " ";
        }
        if let Some(ref v) = self.url {
            write!(f, "{}{}", spacing, v.blue())?;
            spacing = " ";
        }
        if let Some(ref v) = self.status {
            write!(f, "{}{}", spacing, v.bold())?;
            spacing = " ";
        }
        if let Some(ref v) = self.elapsed {
            write!(f, "{}{}", spacing, v.bold())?;
            spacing = " ";
        }
        if let Some(ref v) = self.bytes {
            write!(f, "{}{}", spacing, v.bold().bright_red())?;
            spacing = " ";
        }
        match self.rest {
            Value::Object(ref map) => {
                map.keys().for_each(|key| {
                    if let Some(v) = map.get(key) {
                        let _ignored = write!(f, "{spacing}{}", v.as_str().unwrap_or_default());
                        spacing = " ";
                    }
                });
            }
            _ => {
                let the_rest = self.rest.as_str().unwrap_or_default();
                write!(f, "{spacing}{the_rest}")?;
            }
        }

        writeln!(f)
    }
}

fn main() -> Result<(), std::io::Error> {
    let mut line = String::new();
    // let mut buffer: Vec<u8> = Vec::with_capacity(1000);

    let mut inlock = io::stdin().lock();
    let mut outlock = io::stdout().lock();

    while inlock.read_line(&mut line)? != 0 {
        // We have a lightweight opinion about log line formats: they have
        // string keys and possibly nested json values.
        match serde_json::from_str::<Message>(line.as_str()) {
            Ok(message) => {
                outlock.write_all(message.to_string().as_bytes())?;
            }
            Err(_) => outlock.write_all(line.as_bytes())?,
        }

        //outlock.write_all(buffer.as_slice())?;
        line.clear();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logline_deser() {
        let logline =
            r#"{"timestamp": "2025-07-30T17:41-07:00","message": "I'm not crazy you're the ones who are crazy"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        println!("{parsed}");
        assert_eq!(parsed.message, "I'm not crazy you're the ones who are crazy");
        let logline = r#"{"timestamp": "2025-07-30T17:41-07:00",
            "message":"I'm not crazy you're the ones who are crazy",
            "request_id":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));

        println!("{parsed}");
    }
}
