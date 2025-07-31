//! Pretty-print newline-delimited json (ndjson) logs.
//! No more, no less.

use std::fmt::Display;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

use anyhow::anyhow;
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::Value;
use tabwriter::TabWriter;

#[derive(Debug, Clone, Parser)]
#[clap(name="bistre", version, styles = v3_styles(), max_term_width = 100)]
/// A tool for pretty-printing json logs or any ndjson content that has
/// timestamp and message fields.
///
/// The timestamp field may be named `time`, `ts`, or `timestamp`. The message
/// field may be named `message` or `msg`. The tool has some opinions about
/// ordering for fields commonly found in server log structures, but will print
/// out every field that shows up in the log line, using the color theme you
/// have set in your terminal.
struct Args {
    /// Follow the file, continuing to watch for more data to arrive.
    #[arg(short, long, global = true)]
    follow: bool,
    /// If prefixed with -, the number of lines from the end to start reading
    /// from. If prefixed with +, the number of lines from the start.
    /// Only makes sense if you're tailing a file.
    #[arg(default_value = "0")]
    offset: String,
    /// Tail the named file; defaults to catting stdin if not provided.
    #[arg(default_value = "")]
    tail: String,
}

/// I like my clap help styled the old way.
fn v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

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

impl Message {
    pub fn write<T>(&self, tabby: &mut TabWriter<T>) -> anyhow::Result<()>
    where
        T: Write,
    {
        write!(
            tabby,
            "{}\t{}",
            self.timestamp.strftime("%F-%T").bright_blue(),
            self.message.bold()
        )?;

        let mut cells: Vec<String> = Vec::new();
        if let Some(ref v) = self.request_id {
            cells.push(format!("reqid: {}", v.bright_yellow()));
        }
        if let Some(ref v) = self.host {
            cells.push(format!("{}", v.green()));
        }
        if let Some(ref v) = self.method {
            cells.push(format!("{}", v.blue()));
        }
        if let Some(ref v) = self.url {
            cells.push(format!("{}", v.blue()));
        }
        if let Some(ref v) = self.status {
            cells.push(format!("{}", v.purple()));
        }
        // todo humanize ? unsure the units here might vary, so maybe not.
        if let Some(ref v) = self.elapsed {
            cells.push(format!("{}", v.bright_purple()));
        }
        // todo humanize ? same comment as above.
        if let Some(ref v) = self.bytes {
            cells.push(format!("{}", v.magenta()));
        }
        match self.rest {
            Value::Object(ref map) => {
                map.iter().for_each(|(key, value)| {
                    cells.push(format!("{key}: {}", value.cyan()));
                });
            }
            _ => {
                let the_rest = self.rest.as_str().unwrap_or_default();
                cells.push(the_rest.to_string());
            }
        }
        let _results: Vec<Result<(), std::io::Error>> = cells
            .iter()
            .enumerate()
            .map(|(i, xs)| {
                if i % 5 == 0 {
                    tabby.flush()?;
                }
                write!(tabby, "{xs}")?;
                Ok(())
            })
            .collect();
        Ok(tabby.flush()?)
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::io::Cursor;
        let buffer = Cursor::new(Vec::with_capacity(1000));
        let mut tabby = TabWriter::new(buffer);

        self.write(&mut tabby).map_err(|_| std::fmt::Error)?;

        let buffy = tabby.into_inner().map_err(|_| std::fmt::Error)?;
        let bytes = buffy.into_inner();

        write!(f, "{}", String::from_utf8_lossy(bytes.as_slice()))
    }
}

fn cat_stdin() -> anyhow::Result<()> {
    let mut line = String::new();
    let mut inlock = io::stdin().lock();
    let outlock = io::stdout().lock();
    let mut tabby = TabWriter::new(outlock).ansi(true).minwidth(10);

    while inlock.read_line(&mut line)? != 0 {
        match serde_json::from_str::<Message>(line.as_str()) {
            Ok(message) => {
                message.write(&mut tabby)?;
                tabby.flush()?;
            }
            Err(_) => {
                tabby.write_all(line.as_bytes())?;
                tabby.flush()?;
            }
        }
        line.clear();
    }

    Ok(())
}

fn tail(fpath: PathBuf, offset: i64) -> anyhow::Result<()>
where
{
    if !fpath.exists() {
        return Err(anyhow!("{} does not exist!", fpath.display()));
    }
    if !fpath.is_file() {
        return Err(anyhow!("{} is not a file!", fpath.display()));
    }
    let fp = match File::open(&fpath) {
        Ok(v) => v,
        Err(e) => {
            return Err(anyhow!("Unable to open {}: {}", fpath.display(), e.to_string()));
        }
    };

    // if the offset is beyond the number of lines in the file
    // in either direction, pin at the beginning or end.
    // if offset is positive, start reading from the beginning.

    let reader = BufReader::new(fp);
    let _lines = if offset > 0 {
        reader.lines().skip(offset as usize)
    } else if offset < 0 {
        // seek to the end of the file
        // seek back offset lines
        // then iterate from there forward
        todo!()
    } else {
        // seek to the end
        todo!()
    };

    // wait for another line ending to be read
    // print, repeat

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // A bit fiddly but: if we see args.offset = "0" and args.tail = empty,
    // we do stdin and stop at EOF. If we see args.offset = some string and
    // args.tail = empty, we tail the given file from the end. Otherwise,
    // we do the tail the named file from the offset thing. We do not yet
    // try to be fancy and tail multiple files. If we will ever do such a thing.

    if args.offset == "0" && args.tail.is_empty() {
        cat_stdin()
    } else if args.tail.is_empty() {
        let fpath = PathBuf::from(args.offset);
        tail(fpath, 0)
    } else {
        let fpath = PathBuf::from(args.tail);
        let offset: i64 = args.offset.parse::<i64>()?;
        tail(fpath, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn logline_deser() {
        let logline =
            r#"{"timestamp": "2025-07-30T17:41-07:00","message": "I'm not crazy you're the ones who are crazy"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.message, "I'm not crazy you're the ones who are crazy");
        let logline = r#"{"timestamp": "2025-07-30T17:41-07:00",
            "message":"I'm not crazy you're the ones who are crazy",
            "request_id":"institutionalized"}"#;
        let parsed = serde_json::from_str::<Message>(logline).expect("this is a valid log message");
        assert_eq!(parsed.request_id, Some("institutionalized".to_string()));
        let logline = r#"{"ts": "2025-07-30T17:41-07:00",
            "msg":"I'm not crazy you're the ones who are crazy",
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
