//! JSON log patterns and deserialization implementations, plus pretty-printing
//! for all identified varieties. Also, traits for all of them, so everything
//! outside this module doesn't have to care what kind of log pattern it is: its
//! just a printable log.

use std::fmt::Display;

use bytes::BytesMut;
use serde::Deserialize;

mod columns;
mod formatting;
mod logfmt;
mod patterns;
mod sourced;

use logfmt::*;
use patterns::*;
pub use sourced::SourcedLine;

/// If you can be pretty-printed, you write into a mutable byte buffer.
/// We don't have any opinions about what you write; pretty is in the eye
/// of the implementor.
pub trait PrettyPrintable
where
    Self: Display,
{
    fn write(&self, buffer: &mut BytesMut) -> usize;
    fn cells(&self) -> Vec<String>;

    /// Default `Display::fmt` body. Renders to a temporary buffer via `write`,
    /// then forwards utf8 chunks to the formatter. Each variant's
    /// `Display::fmt` impl is just `self.fmt_pretty(f)`.
    fn fmt_pretty(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = BytesMut::with_capacity(2048);
        self.write(&mut buffer);
        buffer
            .utf8_chunks()
            .try_for_each(|chunk| write!(f, "{}", chunk.valid()))
    }
}

/// An enum to help serde deserialize incoming log lines. There are some
/// we decide are log lines with fields we recognize, and some that are just
/// json we pretty-print. And then there's plain text.
/// Ordered from most specific to least specific for serde deserialization.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, bound(deserialize = "'de: 'a"))]
pub enum Printable<'a> {
    Canonical(Box<Canonical<'a>>),
    Java(Box<Java<'a>>),
    Message(Box<Message<'a>>),
    TimeOnly(Timestamped),
    Json(GenericJson),
    Logfmt(LogfmtLine),
    Text(String),
}

impl<'a> PrettyPrintable for Printable<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        match self {
            Printable::Canonical(canonical) => canonical.as_ref().write(buffer),
            Printable::Java(java) => java.as_ref().write(buffer),
            Printable::Message(message) => message.as_ref().write(buffer),
            Printable::TimeOnly(timestamped) => timestamped.write(buffer),
            Printable::Json(generic) => generic.write(buffer),
            Printable::Logfmt(logfmt) => logfmt.write(buffer),
            Printable::Text(text) => {
                buffer.extend_from_slice(text.as_bytes());
                buffer.len()
            }
        }
    }

    fn cells(&self) -> Vec<String> {
        match self {
            Printable::Canonical(canonical) => canonical.as_ref().cells(),
            Printable::Java(java) => java.as_ref().cells(),
            Printable::Message(message) => message.as_ref().cells(),
            Printable::TimeOnly(timestamped) => timestamped.cells(),
            Printable::Json(generic) => generic.cells(),
            Printable::Logfmt(logfmt) => logfmt.cells(),
            Printable::Text(_) => Vec::new(),
        }
    }
}

impl<'a> Display for Printable<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Printable::Canonical(canonical) => canonical.fmt(f),
            Printable::Java(java) => java.fmt(f),
            Printable::Message(message) => message.fmt(f),
            Printable::TimeOnly(timestamped) => timestamped.fmt(f),
            Printable::Json(generic) => generic.fmt(f),
            Printable::Logfmt(logfmt) => logfmt.fmt(f),
            Printable::Text(text) => text.fmt(f),
        }
    }
}
