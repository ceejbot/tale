//! The logging patterns themselves, with implementations.
//! They have to be deserialized from most specific to least
//! specific but they're in the opposite order in this file.

use std::borrow::Cow;
use std::fmt::{self, Display};

use bytes::BytesMut;
use humansize::{BINARY, format_size};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use serde_json::Value;

use super::PrettyPrintable;
use super::formatting::*;

/// Deserialize a JSON value that may be either a string or a number into
/// `Option<Cow<'de, str>>`. Many real-world log producers (nginx, k8s, GCP)
/// emit numeric values for fields like `status` and `size` that we want to
/// surface as text. Without this, those producers' lines silently fall back
/// to the GenericJson rendering path. Numeric values are formatted via
/// their `Display` impl and allocated as `Cow::Owned`; strings stay borrowed.
fn de_string_or_number<'de, D>(deserializer: D) -> Result<Option<Cow<'de, str>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<Cow<'de, str>>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a string, number, or null")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(Cow::Owned(v.to_owned())))
        }
        fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
            Ok(Some(Cow::Borrowed(v)))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(Cow::Owned(v)))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(Cow::Owned(v.to_string())))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(Cow::Owned(v.to_string())))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(Cow::Owned(v.to_string())))
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(V)
        }
    }

    deserializer.deserialize_any(V)
}

/// Walk a `#[serde(flatten)]` rest field and emit one cell per entry.
/// Object → one cell per key; the non-Object fallback (defensive — flatten
/// always produces an object in practice) labels the value with key "rest".
fn rest_to_cells(rest: &Value) -> Vec<String> {
    match rest {
        Value::Object(map) => map.iter().map(|(k, v)| colorize_map_entry(k, v)).collect(),
        v => vec![colorize_map_entry("rest", v)],
    }
}

/// We couldn't parse this json as a log line, even minimally, but it's
/// json so we'll print it.
#[derive(Debug, Clone, Deserialize)]
pub struct GenericJson {
    #[serde(flatten)]
    pub(super) rest: serde_json::Value,
}

impl PrettyPrintable for &GenericJson {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let metrics = LayoutMetrics::current();

        buffer.extend_from_slice(JSON_HEADER);
        if metrics.show_time {
            // Pad past the (omitted) timestamp column so cells start at the
            // same column they'd start at if a timestamp were shown.
            pad_spaces(buffer, MODULE_WIDTH);
        }
        buffer.extend_from_slice(COL_SEP.as_bytes());

        write_rest(*self, buffer, metrics.max_message_width, metrics.padding)
    }

    fn cells(&self) -> Vec<String> {
        rest_to_cells(&self.rest)
    }
}

impl Display for GenericJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}

/// The *only* thing we could recognize here was a timestamp.
#[derive(Debug, Clone, Deserialize)]
pub struct Timestamped {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    /// Everything else.
    #[serde(flatten)]
    pub(super) rest: serde_json::Value,
}

impl Display for Timestamped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}

impl PrettyPrintable for &Timestamped {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let metrics = LayoutMetrics::current();

        buffer.extend_from_slice(JSON_HEADER);
        if metrics.show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        write_rest(*self, buffer, metrics.max_message_width, metrics.padding)
    }

    fn cells(&self) -> Vec<String> {
        rest_to_cells(&self.rest)
    }
}

/// Optimized superset JSON log structure with field clustering for performance.
/// Fields are ordered by parsing frequency: core fields first, then HTTP (most
/// common), then specialized contexts only when needed.
#[derive(Debug, Clone, Deserialize)]
pub struct Message<'a> {
    /// Primary timestamp field (supports multiple common names and formats)
    #[serde(alias = "time", alias = "ts", alias = "@timestamp", alias = "time_local")]
    pub(crate) timestamp: Option<jiff::Timestamp>,

    /// The string message part of the log line (required field)
    #[serde(alias = "msg", alias = "body", borrow)]
    pub(crate) message: Cow<'a, str>,

    /// Log level/severity
    #[serde(alias = "lvl", alias = "severity", borrow)]
    pub(crate) level: Cow<'a, str>,

    /// HTTP method
    #[serde(alias = "requestMethod", borrow)]
    pub(crate) method: Option<Cow<'a, str>>,
    /// Request URL
    #[serde(alias = "requestUrl", borrow)]
    pub(crate) url: Option<Cow<'a, str>>,
    /// HTTP response status code (string or numeric in source JSON).
    #[serde(
        default,
        alias = "statusCode",
        alias = "code",
        borrow,
        deserialize_with = "de_string_or_number"
    )]
    pub(crate) status: Option<Cow<'a, str>>,
    /// Request/response elapsed time
    #[serde(alias = "elapsed_ms", alias = "elapsed_time", alias = "latency", borrow)]
    pub(crate) elapsed: Option<Cow<'a, str>>,
    /// Response size (string or numeric in source JSON).
    #[serde(
        default,
        alias = "sent_bytes",
        alias = "length",
        alias = "written",
        alias = "bodylen",
        alias = "size",
        alias = "responseSize",
        borrow,
        deserialize_with = "de_string_or_number"
    )]
    pub(crate) size: Option<Cow<'a, str>>,

    // === Request Tracking Fields (Common - third priority) ===
    /// Request/trace identifier
    #[serde(alias = "requestId", alias = "request_id", alias = "requestid", borrow)]
    pub(crate) request_id: Option<Cow<'a, str>>,
    /// Thread name/identifier
    #[serde(alias = "threadName", alias = "thread_name", alias = "thread_id", borrow)]
    pub(crate) thread: Option<Cow<'a, str>>,
    /// Remote host/IP
    #[serde(
        alias = "hostname",
        alias = "remote_host",
        alias = "remoteIp",
        alias = "source_host",
        borrow
    )]
    pub(crate) host: Option<Cow<'a, str>>,
    /// User agent
    #[serde(alias = "userAgent", borrow)]
    pub(crate) user_agent: Option<Cow<'a, str>>,

    // === Source Location Fields (Moderately common) ===
    /// The source module/class where the log line originated
    #[serde(
        alias = "mod",
        alias = "lib",
        alias = "class",
        alias = "className",
        alias = "loggerName",
        borrow
    )]
    pub(crate) module: Option<Cow<'a, str>>,
    /// The file where the log line originated
    #[serde(alias = "fileName", alias = "file_name", borrow)]
    pub(crate) file: Option<Cow<'a, str>>,
    /// The line in the file where the log line originated
    #[serde(alias = "lineNumber", alias = "line_number")]
    pub(crate) line: Option<usize>,
    /// Method/function name
    #[serde(alias = "methodName", alias = "method_name", borrow)]
    pub(crate) function: Option<Cow<'a, str>>,

    // === Web Server Performance Fields (grouped for performance) ===
    /// Response body bytes sent (string or numeric in source JSON).
    #[serde(default, alias = "body_bytes_sent", borrow, deserialize_with = "de_string_or_number")]
    pub(crate) response_bytes: Option<Cow<'a, str>>,
    /// Total request processing time (string or numeric in source JSON).
    #[serde(default, alias = "request_time", borrow, deserialize_with = "de_string_or_number")]
    pub(crate) request_duration: Option<Cow<'a, str>>,
    /// Backend response time (string or numeric in source JSON).
    #[serde(
        default,
        alias = "upstream_response_time",
        borrow,
        deserialize_with = "de_string_or_number"
    )]
    pub(crate) upstream_time: Option<Cow<'a, str>>,
    /// Backend header time (string or numeric in source JSON).
    #[serde(
        default,
        alias = "upstream_header_time",
        borrow,
        deserialize_with = "de_string_or_number"
    )]
    pub(crate) upstream_header_time: Option<Cow<'a, str>>,
    /// Backend server address
    #[serde(alias = "upstream_addr", borrow)]
    pub(crate) upstream_server: Option<Cow<'a, str>>,
    /// Backend response status (string or numeric in source JSON).
    #[serde(default, alias = "upstream_status", borrow, deserialize_with = "de_string_or_number")]
    pub(crate) upstream_status: Option<Cow<'a, str>>,
    /// Request URI/path
    #[serde(alias = "uri", borrow)]
    pub(crate) path: Option<Cow<'a, str>>,
    /// Query string arguments
    #[serde(alias = "args", borrow)]
    pub(crate) query_args: Option<Cow<'a, str>>,
    /// Request content length (string or numeric in source JSON).
    #[serde(default, alias = "request_length", borrow, deserialize_with = "de_string_or_number")]
    pub(crate) request_size: Option<Cow<'a, str>>,

    // === Tracing/Observability Fields (grouped for performance) ===
    /// Distributed trace ID
    #[serde(alias = "traceId", borrow)]
    pub(crate) trace_id: Option<Cow<'a, str>>,
    /// Span ID within trace
    #[serde(alias = "spanId", borrow)]
    pub(crate) span_id: Option<Cow<'a, str>>,
    /// Trace flags for sampling
    #[serde(alias = "traceFlags", borrow)]
    pub(crate) trace_flags: Option<Cow<'a, str>>,
    /// OpenTelemetry resource metadata
    pub(crate) resource: Option<serde_json::Value>,

    // k8s fields grouped for locality
    /// Docker log line content
    #[serde(borrow)]
    pub(crate) log: Option<Cow<'a, str>>,
    /// Docker stream designation (stdout/stderr)
    #[serde(borrow)]
    pub(crate) stream: Option<Cow<'a, str>>,
    /// Kubernetes pod name
    #[serde(alias = "pod_name", borrow)]
    pub(crate) pod: Option<Cow<'a, str>>,
    /// Kubernetes namespace
    #[serde(borrow)]
    pub(crate) namespace: Option<Cow<'a, str>>,
    /// Container name
    #[serde(alias = "container_name", borrow)]
    pub(crate) container: Option<Cow<'a, str>>,
    /// Kubernetes node name
    #[serde(alias = "node_name", borrow)]
    pub(crate) node: Option<Cow<'a, str>>,

    /// Catch-all for any additional fields not explicitly handled
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
}

impl<'a> Display for Message<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}

impl<'a> PrettyPrintable for &Message<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let metrics = LayoutMetrics::current();
        // Let's get absurd! Who wants to alloc, anyway?
        buffer.extend_from_slice(get_level_bytes(&self.level));

        if metrics.show_time {
            if let Some(ref v) = self.timestamp {
                // Add space + formatted timestamp + separator
                buffer.extend_from_slice(b" ");
                write_timestamp_column(buffer, v);
            } else if let Some(ref v) = self.request_id {
                // Add space + formatted request_id + separator
                buffer.extend_from_slice(b" ");
                write_request_id_column(buffer, v);
            } else {
                // Add space + padding + separator
                buffer.extend_from_slice(b" ");
                write_empty_module_column(buffer);
            }
        } else {
            // Just add the separator
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, metrics.padding, metrics.max_message_width);

        // Now we walk through all the other fields, treating some specially,
        // building our list of cells somewhat inefficiently. We pad everything
        // out to the ideal column width as we go, to avoid having to work to
        // avoid counting ansi escapes.
        let mut cells: Vec<String> = Vec::new();

        // Build comprehensive location information from multiple possible sources
        let mut location_parts = Vec::new();

        // Module/class information
        if let Some(ref module) = self.module {
            location_parts.push(module.as_ref().to_string());
        }

        // Function/method information
        if let Some(ref function) = self.function {
            if !location_parts.is_empty() {
                location_parts.push("::".to_string());
            }
            location_parts.push(function.as_ref().to_string());
        }

        // File and line information
        if let Some(ref file) = self.file {
            if !location_parts.is_empty() {
                location_parts.push(" (".to_string());
            }
            location_parts.push(file.as_ref().to_string());
            if let Some(line) = self.line {
                location_parts.push(format!(":{}", line));
            }
            if !location_parts.is_empty() && location_parts.last() != Some(&" (".to_string()) {
                location_parts.push(")".to_string());
            }
        }

        if !location_parts.is_empty() {
            cells.push(colorize_map_entry("location", &Value::String(location_parts.join(""))));
        }

        // if we didn't already snag the request id
        if !metrics.show_time
            && let Some(ref v) = self.request_id
        {
            cells.push(colorize_map_entry("reqid", &Value::String(v.to_string())));
        }

        let status = if let Some(ref v) = self.status {
            // Transform numeric status codes to include reason phrase
            match v.parse::<u16>() {
                Ok(200) => "200 OK".to_string(),
                Ok(201) => "201 Created".to_string(),
                Ok(204) => "204 No Content".to_string(),
                Ok(400) => "400 Bad Request".to_string(),
                Ok(401) => "401 Unauthorized".to_string(),
                Ok(403) => "403 Forbidden".to_string(),
                Ok(404) => "404 Not Found".to_string(),
                Ok(405) => "405 Method Not Allowed".to_string(),
                Ok(500) => "500 Internal Server Error".to_string(),
                Ok(502) => "502 Bad Gateway".to_string(),
                Ok(503) => "503 Service Unavailable".to_string(),
                _ => v.to_string(), // Keep original if not a recognized code
            }
        } else {
            String::default()
        };

        // Special treatment for verb url status
        let mut req_line = if let Some(ref method) = self.method
            && let Some(ref url) = self.url
        {
            format!("{method} {url} {status}")
        } else if let Some(ref url) = self.url {
            format!("{url}{status}")
        } else {
            status
        };
        let has_req_line = !req_line.is_empty();
        if has_req_line {
            if let Some(ref query_args) = self.query_args {
                req_line = format!("{req_line}&{query_args}");
            }
            cells.push(colorize_map_entry("request", &Value::String(req_line)));
        }

        if let Some(ref path) = self.path {
            let full = if !has_req_line && let Some(ref query_args) = self.query_args {
                format!("{path}&{query_args}")
            } else {
                path.to_string()
            };
            cells.push(colorize_map_entry("path", &Value::String(full)));
        } else if !has_req_line && let Some(ref query_args) = self.query_args {
            cells.push(colorize_map_entry("args", &Value::String(query_args.to_string())));
        }

        if let Some(ref request_size) = self.request_size {
            cells.push(colorize_map_entry(
                "request_size",
                &Value::String(format!("{} bytes", request_size)),
            ));
        }

        // Docker/Kubernetes container information
        if let Some(ref log_content) = self.log {
            cells.push(colorize_map_entry("log", &Value::String(log_content.to_string())));
        }

        if let Some(ref stream) = self.stream {
            cells.push(colorize_map_entry("stream", &Value::String(stream.to_string())));
        }

        if let Some(ref pod) = self.pod {
            cells.push(colorize_map_entry("pod", &Value::String(pod.to_string())));
        }

        if let Some(ref namespace) = self.namespace {
            cells.push(colorize_map_entry("namespace", &Value::String(namespace.to_string())));
        }

        if let Some(ref container) = self.container {
            cells.push(colorize_map_entry("container", &Value::String(container.to_string())));
        }

        if let Some(ref node) = self.node {
            cells.push(colorize_map_entry("node", &Value::String(node.to_string())));
        }

        // Web server performance metrics
        if let Some(ref response_bytes) = self.response_bytes {
            cells.push(colorize_map_entry(
                "response_bytes",
                &Value::String(response_bytes.to_string()),
            ));
        }

        if let Some(ref request_duration) = self.request_duration {
            cells.push(colorize_map_entry(
                "request_time",
                &Value::String(request_duration.to_string()),
            ));
        }

        if let Some(ref upstream_time) = self.upstream_time {
            cells.push(colorize_map_entry(
                "upstream_time",
                &Value::String(upstream_time.to_string()),
            ));
        }

        if let Some(ref upstream_server) = self.upstream_server {
            cells.push(colorize_map_entry(
                "upstream",
                &Value::String(upstream_server.to_string()),
            ));
        }

        if let Some(ref upstream_status) = self.upstream_status {
            cells.push(colorize_map_entry(
                "upstream_status",
                &Value::String(upstream_status.to_string()),
            ));
        }

        if let Some(ref upstream_header_time) = self.upstream_header_time {
            cells.push(colorize_map_entry(
                "upstream_header_time",
                &Value::String(format!("{}ms", upstream_header_time)),
            ));
        }

        // OpenTelemetry tracing
        if let Some(ref trace_id) = self.trace_id {
            cells.push(colorize_map_entry("trace_id", &Value::String(trace_id.to_string())));
        }

        if let Some(ref span_id) = self.span_id {
            cells.push(colorize_map_entry("span_id", &Value::String(span_id.to_string())));
        }

        if let Some(ref trace_flags) = self.trace_flags {
            cells.push(colorize_map_entry(
                "trace_flags",
                &Value::String(trace_flags.to_string()),
            ));
        }

        if let Some(ref resources) = self.resource {
            cells.push(colorize_json_value(resources));
        }

        // Thread information
        if let Some(ref thread) = self.thread {
            cells.push(colorize_map_entry("thread", &Value::String(thread.to_string())));
        }

        // Host/IP information
        if let Some(ref host) = self.host {
            cells.push(colorize_map_entry("host", &Value::String(host.to_string())));
        }

        // User agent
        if let Some(ref user_agent) = self.user_agent {
            cells.push(colorize_map_entry("user_agent", &Value::String(user_agent.to_string())));
        }

        // Performance metrics
        if let Some(ref elapsed) = self.elapsed {
            cells.push(colorize_map_entry("elapsed", &Value::String(elapsed.to_string())));
        }

        if let Some(ref size) = self.size {
            cells.push(colorize_map_entry("size", &Value::String(size.to_string())));
        }

        cells.extend(rest_to_cells(&self.rest));

        write_cells_with_padding(buffer, &cells, &metrics);

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        rest_to_cells(&self.rest)
    }
}

/// This is something log4j or slf4j might emit. One of those.
#[derive(Debug, Clone, Deserialize)]
pub struct Java<'a> {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    #[serde(alias = "msg")]
    pub(super) message: Cow<'a, str>,
    #[serde(alias = "severity")]
    pub(super) level: Cow<'a, str>,
    #[serde(alias = "threadName")]
    thread: Cow<'a, str>,
    #[serde(alias = "requestid", alias = "requestId")]
    request_id: Cow<'a, str>,
    class: Cow<'a, str>,
    method: Cow<'a, str>,
    source: Cow<'a, str>,
    #[serde(rename = "stackTrace")]
    stack_trace: Cow<'a, str>,
    #[serde(flatten)]
    pub(crate) rest: serde_json::Value,
}

impl<'a> PrettyPrintable for Java<'a> {
    fn write(&self, buffer: &mut BytesMut) -> usize {
        let metrics = LayoutMetrics::current();

        buffer.extend_from_slice(get_level_bytes(&self.level));
        if metrics.show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, metrics.padding, metrics.max_message_width);

        let mut cells: Vec<String> = Vec::new();

        if !self.request_id.is_empty() {
            cells.push(colorize_map_entry("reqid", &Value::String(self.request_id.to_string())));
        }
        if !self.thread.is_empty() {
            cells.push(colorize_map_entry("thread", &Value::String(self.thread.to_string())));
        }
        cells.push(colorize_map_entry(
            "class",
            &Value::String(format!("{} :: {}", self.class, self.method)),
        ));
        cells.push(colorize_map_entry("source", &Value::String(self.source.to_string())));
        cells.extend(rest_to_cells(&self.rest));

        write_cells_with_padding(buffer, &cells, &metrics);

        if !self.stack_trace.is_empty() {
            pad_spaces(buffer, metrics.padding);
            buffer.extend_from_slice(COL_SEP.as_bytes());
            format_message(&self.stack_trace, buffer, metrics.padding, metrics.max_message_width);
        }

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        rest_to_cells(&self.rest)
    }
}

impl<'a> Display for Java<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}

/// This is a possibly familiar format that we require conformance to.
/// https://brandur.org/canonical-log-lines
#[derive(Debug, Clone, Deserialize)]
pub struct Canonical<'a> {
    /// The time this message was logged.
    #[serde(alias = "time", alias = "ts")]
    pub(super) timestamp: jiff::Timestamp,
    pub(super) level: Cow<'a, str>,
    pub(super) message: Cow<'a, str>,
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
        let metrics = LayoutMetrics::current();

        buffer.extend_from_slice(get_level_bytes(&self.level));
        if metrics.show_time {
            buffer.extend_from_slice(b" ");
            write_timestamp_column(buffer, &self.timestamp);
        } else {
            buffer.extend_from_slice(COL_SEP.as_bytes());
        }

        format_message(&self.message, buffer, metrics.padding, metrics.max_message_width);

        let sized = format_size(self.size, BINARY);
        let mut cells: Vec<String> = vec![
            colorize_map_entry("reqid", &Value::String(self.request_id.to_string())),
            colorize_map_entry("method", &Value::String(self.method.to_string())),
            colorize_map_entry("url", &Value::String(self.url.to_string())),
            colorize_map_entry("status", &Value::Number(serde_json::Number::from(self.status))),
            colorize_map_entry("size", &Value::String(sized)),
            colorize_map_entry("elapsed", &Value::String(self.elapsed.to_string())),
            colorize_map_entry("remote_host", &Value::String(self.remote_host.to_string())),
            colorize_map_entry("user_agent", &Value::String(self.user_agent.to_string())),
        ];
        cells.extend(rest_to_cells(&self.rest));

        write_cells_with_padding(buffer, &cells, &metrics);

        buffer.len()
    }

    fn cells(&self) -> Vec<String> {
        rest_to_cells(&self.rest)
    }
}

impl<'a> Display for Canonical<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_pretty(f)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use jiff::Timestamp;

    use super::*;
    use crate::config::{self, ConfigOpts};
    use crate::logpatterns::{Printable, SourcedLine};

    #[test]
    fn layout_one() {
        // Use with_config to isolate this test
        config::with_config(ConfigOpts::default(), || {
            let logline = r##"{
                "timestamp": "2025-08-01T10:45:03Z",
                "level": "CRITICAL",
                "message": "Database query failed",
                "query": "SELECT * FROM users WHERE id = ?",
                "error_code": "ER_NO_SUCH_TABLE",
                "elapsed": "250ms"
            }"##;
            let parsed = serde_json::from_str::<Message<'_>>(logline).expect("this is a valid log message");
            let stringy = parsed.to_string();
            let lines: Vec<&str> = stringy.split('\n').collect();
            let length = lines.len();

            assert_eq!(length, 5);
        });
    }

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
            panic!("Expected Canonical variant, got: {:#?}", parsed)
        };
        assert_eq!(canonical.message, "HTTP PATCH /api/auth/login");
    }

    #[test]
    fn log4j_stuff() {
        let logline = r#"{
            "timestamp":"2025-07-30T10:42:24.983Z",
            "message":"I like drinking coffee in the morning.",
            "level":"INFO",
            "threadName":"main",
            "requestid":"59F7A6B7-3EA3-4465-A4AE-CA3010F60695",
            "class":"com.sprockets.widgets.Dispatcher",
            "method":"dispatchWidget",
            "source":"Dispatcher.scala:11",
            "stackTrace":""
        }"#;
        let parsed =
            serde_json::from_str::<Printable<'_>>(logline).expect("the HTTP patch message is a valid log line");
        let Printable::Java(java) = parsed else {
            panic!("Expected Java variant, got: {:#?}", parsed)
        };
        assert_eq!(java.message, "I like drinking coffee in the morning.");
    }

    #[test]
    fn stable_sort_single_file_no_timestamps() {
        use std::path::PathBuf;

        // Lines from a single file with no timestamps - should maintain original order
        let lines = [
            (PathBuf::from("test.log"), 0, r#"{"message": "third line"}"#.to_string()),
            (PathBuf::from("test.log"), 1, r#"{"message": "first line"}"#.to_string()),
            (
                PathBuf::from("test.log"),
                2,
                r#"{"message": "second line"}"#.to_string(),
            ),
        ];
        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Should maintain original order since no timestamps and same file
        assert_eq!(sorted[0].line_number, 0);
        assert_eq!(sorted[1].line_number, 1);
        assert_eq!(sorted[2].line_number, 2);
    }

    #[test]
    fn docker_json_log_format() {
        // Test Docker JSON logging driver format
        let logline = r#"{
            "log": "Error: database connection failed\n",
            "stream": "stderr",
            "time": "2025-08-16T14:30:25.123456789Z",
            "message": "Container error log",
            "level": "ERROR"
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("Docker log should parse");
        assert_eq!(
            parsed.log.as_ref().expect("text fixtures must work"),
            "Error: database connection failed\n"
        );
        assert_eq!(parsed.stream.as_ref().expect("text fixtures must work"), "stderr");
        assert_eq!(parsed.message, "Container error log");
        assert_eq!(parsed.level, "ERROR");
        assert!(parsed.timestamp.is_some());
    }

    #[test]
    fn kubernetes_structured_log_format() {
        // Test Kubernetes CRI/structured logging format
        let logline = r#"{
            "ts": "2025-08-16T15:20:00.123456Z",
            "msg": "Pod status updated",
            "level": "INFO",
            "pod": "coredns-558bd4d5db-xyz123",
            "namespace": "kube-system",
            "container": "coredns",
            "node": "worker-node-1"
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("Kubernetes log should parse");
        assert_eq!(parsed.message, "Pod status updated");
        assert_eq!(parsed.level, "INFO");
        assert_eq!(
            parsed.pod.as_ref().expect("text fixtures must work"),
            "coredns-558bd4d5db-xyz123"
        );
        assert_eq!(
            parsed.namespace.as_ref().expect("text fixtures must work"),
            "kube-system"
        );
        assert_eq!(parsed.container.as_ref().expect("text fixtures must work"), "coredns");
        assert_eq!(parsed.node.as_ref().expect("text fixtures must work"), "worker-node-1");
        assert!(parsed.timestamp.is_some());
    }

    #[test]
    fn numeric_status_and_size_parse_into_message() {
        // Real-world nginx/k8s logs often emit `status` and `size` as numbers,
        // not strings. Before the de_string_or_number helper, this fell
        // through to the GenericJson path and lost the HTTP-aware rendering.
        let logline = r#"{
            "level": "INFO",
            "message": "request",
            "method": "GET",
            "url": "/foo",
            "status": 404,
            "size": 1234,
            "body_bytes_sent": 999,
            "request_length": 256,
            "upstream_response_time": 0.42,
            "upstream_header_time": 0.05,
            "upstream_status": 200,
            "request_time": 0.5
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("numeric fields should parse");
        assert_eq!(parsed.status.as_deref(), Some("404"));
        assert_eq!(parsed.size.as_deref(), Some("1234"));
        assert_eq!(parsed.response_bytes.as_deref(), Some("999"));
        assert_eq!(parsed.request_size.as_deref(), Some("256"));
        assert_eq!(parsed.upstream_time.as_deref(), Some("0.42"));
        assert_eq!(parsed.upstream_header_time.as_deref(), Some("0.05"));
        assert_eq!(parsed.upstream_status.as_deref(), Some("200"));
        assert_eq!(parsed.request_duration.as_deref(), Some("0.5"));
    }

    #[test]
    fn nginx_json_access_log_format() {
        // Test Nginx JSON access log format
        let logline = r#"{
            "timestamp": "2025-08-16T14:21:45Z",
            "remote_addr": "127.0.0.1",
            "method": "GET",
            "uri": "/api/users",
            "status": "200",
            "body_bytes_sent": "1234",
            "request_time": "0.123",
            "upstream_response_time": "0.100",
            "upstream_addr": "backend:8080",
            "upstream_status": "200",
            "args": "limit=10&offset=20",
            "message": "Access log entry",
            "level": "INFO"
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("Nginx log should parse");
        assert_eq!(parsed.message, "Access log entry");
        assert_eq!(parsed.level, "INFO");
        assert_eq!(parsed.method.as_ref().expect("text fixtures must work"), "GET");
        assert_eq!(parsed.path.as_ref().expect("text fixtures must work"), "/api/users");
        assert_eq!(parsed.status.as_ref().expect("text fixtures must work"), "200");
        assert_eq!(parsed.response_bytes.as_ref().expect("text fixtures must work"), "1234");
        assert_eq!(
            parsed.request_duration.as_ref().expect("text fixtures must work"),
            "0.123"
        );
        assert_eq!(parsed.upstream_time.as_ref().expect("text fixtures must work"), "0.100");
        assert_eq!(
            parsed.upstream_server.as_ref().expect("text fixtures must work"),
            "backend:8080"
        );
        assert_eq!(parsed.upstream_status.as_ref().expect("text fixtures must work"), "200");
        assert_eq!(
            parsed.query_args.as_ref().expect("text fixtures must work"),
            "limit=10&offset=20"
        );
        assert!(parsed.timestamp.is_some());
    }

    #[test]
    fn opentelemetry_log_format() {
        // Test OpenTelemetry log format with tracing
        let logline = r#"{
            "timestamp": "2025-08-16T14:00:00Z",
            "traceId": "abc123def456789",
            "spanId": "def456abc123",
            "traceFlags": "01",
            "level": "INFO",
            "body": "Operation completed successfully",
            "resource": {"service.name": "api-server", "service.version": "1.0.0"}
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("OpenTelemetry log should parse");
        assert!(parsed.timestamp.is_some());
        assert_eq!(
            parsed.trace_id.as_ref().expect("text fixtures must work"),
            "abc123def456789"
        );
        assert_eq!(
            parsed.span_id.as_ref().expect("text fixtures must work"),
            "def456abc123"
        );
        assert_eq!(parsed.trace_flags.as_ref().expect("text fixtures must work"), "01");
        assert_eq!(parsed.level, "INFO");
        assert_eq!(parsed.message, "Operation completed successfully");
        assert!(parsed.resource.is_some());
    }

    #[test]
    fn docker_kubernetes_mixed_log() {
        // Test log that combines Docker and Kubernetes fields
        let logline = r#"{
            "time": "2025-08-16T14:30:00Z",
            "log": "Starting application server\n",
            "stream": "stdout",
            "message": "Container startup",
            "level": "INFO",
            "pod_name": "app-deployment-abc123",
            "namespace": "production",
            "container_name": "app-server",
            "node_name": "k8s-worker-3"
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("Mixed Docker/K8s log should parse");
        assert_eq!(parsed.message, "Container startup");
        assert_eq!(parsed.level, "INFO");
        assert_eq!(
            parsed.log.as_ref().expect("text fixtures must work"),
            "Starting application server\n"
        );
        assert_eq!(parsed.stream.as_ref().expect("text fixtures must work"), "stdout");
        assert_eq!(
            parsed.pod.as_ref().expect("text fixtures must work"),
            "app-deployment-abc123"
        );
        assert_eq!(
            parsed.namespace.as_ref().expect("text fixtures must work"),
            "production"
        );
        assert_eq!(
            parsed.container.as_ref().expect("text fixtures must work"),
            "app-server"
        );
        assert_eq!(parsed.node.as_ref().expect("text fixtures must work"), "k8s-worker-3");
        assert!(parsed.timestamp.is_some());
    }

    #[test]
    fn web_server_performance_log() {
        // Test web server log with extensive performance metrics
        let logline = r#"{
            "timestamp": "2025-08-16T14:45:30Z",
            "message": "HTTP request processed",
            "level": "INFO",
            "method": "POST",
            "url": "/api/orders",
            "status": "201",
            "request_time": "0.485",
            "upstream_response_time": "0.420",
            "upstream_header_time": "0.050",
            "upstream_addr": "backend1:3000,backend2:3000",
            "upstream_status": "201,201",
            "body_bytes_sent": "2048",
            "request_length": "512",
            "remote_host": "192.168.1.100",
            "user_agent": "Mozilla/5.0 (compatible; API-Client/1.0)"
        }"#;
        let parsed = serde_json::from_str::<Message<'_>>(logline).expect("Performance log should parse");
        assert_eq!(parsed.message, "HTTP request processed");
        assert_eq!(parsed.method.as_ref().expect("text fixtures must work"), "POST");
        assert_eq!(parsed.url.as_ref().expect("text fixtures must work"), "/api/orders");
        assert_eq!(parsed.status.as_ref().expect("text fixtures must work"), "201");
        assert_eq!(
            parsed.request_duration.as_ref().expect("text fixtures must work"),
            "0.485"
        );
        assert_eq!(parsed.upstream_time.as_ref().expect("text fixtures must work"), "0.420");
        assert_eq!(
            parsed.upstream_header_time.as_ref().expect("text fixtures must work"),
            "0.050"
        );
        assert_eq!(
            parsed.upstream_server.as_ref().expect("text fixtures must work"),
            "backend1:3000,backend2:3000"
        );
        assert_eq!(
            parsed.upstream_status.as_ref().expect("text fixtures must work"),
            "201,201"
        );
        assert_eq!(parsed.response_bytes.as_ref().expect("test fixtures must work"), "2048");
        assert_eq!(parsed.request_size.as_ref().expect("test fixtures must work"), "512");
    }

    #[test]
    fn stable_sort_single_file_with_same_timestamps() {
        use std::path::PathBuf;

        // Lines with identical timestamps - should maintain original order (stable
        // sort)
        let lines = [
            (
                PathBuf::from("test.log"),
                0,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "first"}"#.to_string(),
            ),
            (
                PathBuf::from("test.log"),
                1,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "second"}"#.to_string(),
            ),
            (
                PathBuf::from("test.log"),
                2,
                r#"{"timestamp": "2025-08-01T10:00:00Z", "message": "third"}"#.to_string(),
            ),
        ];

        let mut sorted: Vec<SourcedLine<'_>> = lines
            .iter()
            .map(|xs| {
                let input = (xs.0.clone(), xs.1 as usize, xs.2.as_str());
                SourcedLine::from(input)
            })
            .collect();
        sorted.sort_by_key(|xs| xs.sort_key());

        // Should maintain original order due to stable sort with identical timestamps
        // first
        let Printable::TimeOnly(ref has_stamp) = sorted[0].parsed else {
            panic!("Expected TimeOnly variant for sorted[0], got: {:#?}", sorted[0].parsed)
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);

        // second
        let Printable::TimeOnly(ref has_stamp) = sorted[1].parsed else {
            panic!("Expected TimeOnly variant for sorted[1], got: {:#?}", sorted[1].parsed)
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);
        let obj = has_stamp.rest.as_object().expect("rest should be a json object");
        let message = obj.get("message").expect("there is a message in this bottle yeah-a");
        assert_eq!(message, "second");

        // third
        let Printable::TimeOnly(ref has_stamp) = sorted[2].parsed else {
            panic!("Expected TimeOnly variant for sorted[2], got: {:#?}", sorted[2].parsed)
        };
        let ts_parsed = Timestamp::from_str("2025-08-01T10:00:00Z").expect("timestamp is parsable");
        assert_eq!(has_stamp.timestamp, ts_parsed);
        let obj = has_stamp.rest.as_object().expect("rest should be a json object");
        assert!(obj.contains_key("message"));
        let message = obj.get("message").expect("there is a message in this bottle yeah-a");
        assert_eq!(message, "third");
    }
}
