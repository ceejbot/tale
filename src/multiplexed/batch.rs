//! Line batching and timestamp sorting for multi-file output.
//!
//! Each incoming line is parsed once into `Printable` and rendered into bytes
//! at `BatchedLine::new` time. The rendered bytes are sorted by timestamp (or
//! by arrival time as a fallback) and emitted in chronological order. This
//! avoids the previous design's flaw of parsing each line twice — once to
//! sniff a timestamp, once to render.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use jiff::Timestamp;
use tokio::sync::mpsc;

use crate::errors::TaleError;
use crate::logpatterns::{PrettyPrintable, Printable, SourcedLine};

/// A line from a file with metadata for sorting and batching, plus
/// pre-rendered output bytes ready to write to stdout.
#[derive(Debug, Clone)]
pub struct BatchedLine {
    /// Pre-rendered output bytes including the trailing blank-line separator.
    pub rendered: BytesMut,
    /// Extracted timestamp (when the line carries one). Primary sort key.
    pub timestamp: Option<Timestamp>,
    /// Wall-clock arrival time. Tiebreaker for non-timestamped lines.
    pub received_at: Instant,
}

impl BatchedLine {
    /// Parse `line` once into a `Printable`, extract its timestamp, and
    /// render the whole thing — including any file-name header — into a
    /// byte buffer. The result is ready to ship to stdout.
    pub fn new(line: String, source_file: PathBuf, line_number: usize) -> Self {
        let mut rendered = BytesMut::with_capacity(2048);
        let timestamp = match serde_json::from_str::<Printable<'_>>(&line) {
            Ok(parsed) => {
                let sourced = SourcedLine::new(parsed, source_file, line_number);
                let ts = sourced.timestamp().copied();
                sourced.write(&mut rendered);
                ts
            }
            Err(_) => {
                // Not valid JSON — render as a plain text line.
                let sourced = SourcedLine::new(Printable::Text(line), source_file, line_number);
                sourced.write(&mut rendered);
                None
            }
        };
        // Blank-line separator between log entries (matches `process_line`).
        rendered.extend_from_slice(b"\n");

        Self {
            rendered,
            timestamp,
            received_at: Instant::now(),
        }
    }

    /// Sort key for chronological ordering across files.
    pub fn sort_key(&self) -> SortKey {
        match &self.timestamp {
            Some(ts) => SortKey::Timestamp(*ts),
            None => SortKey::ReceivedAt(self.received_at),
        }
    }
}

/// Key used for sorting batched lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    Timestamp(Timestamp),
    ReceivedAt(Instant),
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (SortKey::Timestamp(a), SortKey::Timestamp(b)) => a.cmp(b),
            (SortKey::ReceivedAt(a), SortKey::ReceivedAt(b)) => a.cmp(b),
            // Timestamped lines come before received-time lines.
            (SortKey::Timestamp(_), SortKey::ReceivedAt(_)) => Ordering::Less,
            (SortKey::ReceivedAt(_), SortKey::Timestamp(_)) => Ordering::Greater,
        }
    }
}

/// Wrapper that flips `BinaryHeap` (max-heap) into a min-heap by reversing
/// the comparison, so `pop()` yields the earliest line first.
#[derive(Debug)]
struct MinHeapLine(BatchedLine);

impl PartialEq for MinHeapLine {
    fn eq(&self, other: &Self) -> bool {
        self.0.sort_key() == other.0.sort_key()
    }
}

impl Eq for MinHeapLine {}

impl PartialOrd for MinHeapLine {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MinHeapLine {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap behavior.
        other.0.sort_key().cmp(&self.0.sort_key())
    }
}

/// Configuration for the batch processor.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// How long to wait before emitting a partial batch.
    pub batch_window: Duration,
    /// Maximum number of lines to hold before forcing a batch emit.
    pub max_batch_size: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_window: Duration::from_millis(250),
            max_batch_size: 200,
        }
    }
}

/// Batch processor that sorts lines by timestamp.
pub struct BatchProcessor {
    config: BatchConfig,
    pending_lines: BinaryHeap<MinHeapLine>,
    receiver: Option<mpsc::UnboundedReceiver<BatchedLine>>,
    sender: Option<mpsc::UnboundedSender<Vec<BatchedLine>>>,
    window_start: Option<Instant>,
}

impl BatchProcessor {
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            pending_lines: BinaryHeap::new(),
            receiver: None,
            sender: None,
            window_start: None,
        }
    }

    /// Start processing batches. Returns the sender for incoming lines and
    /// the receiver for sorted output batches.
    pub async fn start(
        &mut self,
    ) -> Result<
        (
            mpsc::UnboundedSender<BatchedLine>,
            mpsc::UnboundedReceiver<Vec<BatchedLine>>,
        ),
        TaleError,
    > {
        let (line_sender, line_receiver) = mpsc::unbounded_channel();
        let (batch_sender, batch_receiver) = mpsc::unbounded_channel();

        // Spawn an owned processor for the task.
        let mut processor = BatchProcessor {
            config: self.config.clone(),
            pending_lines: BinaryHeap::new(),
            receiver: Some(line_receiver),
            sender: Some(batch_sender),
            window_start: None,
        };

        tokio::spawn(async move {
            if let Err(e) = processor.process_loop().await {
                eprintln!("Batch processor error: {e}");
            }
        });

        Ok((line_sender, batch_receiver))
    }

    async fn process_loop(&mut self) -> Result<(), TaleError> {
        let Some(mut line_receiver) = self.receiver.take() else {
            return Err(TaleError::LineReceiver);
        };
        let Some(batch_sender) = self.sender.take() else {
            return Err(TaleError::BatchSender);
        };

        let batch_timeout = tokio::time::interval(self.config.batch_window);
        tokio::pin!(batch_timeout);

        loop {
            tokio::select! {
                line_opt = line_receiver.recv() => {
                    match line_opt {
                        Some(line) => {
                            self.pending_lines.push(MinHeapLine(line));
                            if self.window_start.is_none() {
                                self.window_start = Some(Instant::now());
                            }
                            if self.pending_lines.len() >= self.config.max_batch_size {
                                self.emit_batch(&batch_sender)?;
                            }
                        }
                        None => {
                            // Channel closed; flush remaining and exit.
                            self.emit_batch(&batch_sender)?;
                            break;
                        }
                    }
                }
                _ = batch_timeout.tick() => {
                    if !self.pending_lines.is_empty() {
                        self.emit_batch(&batch_sender)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn emit_batch(&mut self, batch_sender: &mpsc::UnboundedSender<Vec<BatchedLine>>) -> Result<(), TaleError> {
        if self.pending_lines.is_empty() {
            return Ok(());
        }

        // Min-heap pop yields lines in sort order.
        let mut sorted_lines = Vec::with_capacity(self.pending_lines.len());
        while let Some(min_line) = self.pending_lines.pop() {
            sorted_lines.push(min_line.0);
        }
        self.window_start = None;

        batch_sender
            .send(sorted_lines)
            .map_err(|_| TaleError::BatchedLineVecSender)
    }
}

/// Create a new batch processor with custom configuration.
pub fn batched_with_config(config: BatchConfig) -> BatchProcessor {
    BatchProcessor::new(config)
}
