//! Line batching and timestamp sorting for multi-file output.
//!
//! This module handles collecting lines from multiple files, sorting them
//! by timestamp, and emitting them in chronological order.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use jiff::Timestamp;
use tokio::sync::mpsc;
// use tokio::time::timeout;

const TIMESTAMP_FIELDS: [&str; 3] = ["timestamp", "time", "ts"];

/// A line from a file with metadata for sorting and batching
#[derive(Debug, Clone)]
pub struct BatchedLine {
    /// The raw line content
    pub content: String,
    /// The parsed JSON value (if parseable)
    pub parsed_json: Option<serde_json::Value>,
    /// The source file path
    pub source_file: PathBuf,
    /// Timestamp extracted from the line (if any)
    pub timestamp: Option<Timestamp>,
    /// When this line was received (fallback for sorting)
    pub received_at: Instant,
    /// Line number within the source file
    pub line_number: u64,
}

impl BatchedLine {
    /// Create a new BatchedLine
    pub fn new(content: String, source_file: PathBuf, line_number: u64) -> Self {
        Self {
            content,
            parsed_json: None,
            source_file,
            timestamp: None,
            received_at: Instant::now(),
            line_number,
        }
    }

    /// Try to parse this line and extract timestamp information
    pub fn parse(&mut self) -> Result<()> {
        // Try to parse as JSON
        match serde_json::from_str::<serde_json::Value>(&self.content) {
            Ok(json_value) => {
                // Extract timestamp if available
                self.timestamp = self.extract_timestamp(&json_value);
                self.parsed_json = Some(json_value);
            }
            Err(_) => {
                // Not valid JSON, leave parsed as None
                self.parsed_json = None;
                self.timestamp = None;
            }
        }

        Ok(())
    }

    /// Extract timestamp from a parsed JSON value
    fn extract_timestamp(&self, json: &serde_json::Value) -> Option<Timestamp> {
        if let Some(obj) = json.as_object() {
            for field in &TIMESTAMP_FIELDS {
                if let Some(ts_value) = obj.get(*field)
                    && let Some(ts_str) = ts_value.as_str()
                    && let Ok(timestamp) = ts_str.parse::<Timestamp>()
                {
                    return Some(timestamp);
                }
            }
        }

        None
    }

    /// Get the sort key for this line (timestamp or received_at)
    pub fn sort_key(&self) -> SortKey {
        if let Some(ts) = &self.timestamp {
            SortKey::Timestamp(*ts)
        } else {
            SortKey::ReceivedAt(self.received_at)
        }
    }
}

/// Key used for sorting batched lines
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// Sort by extracted timestamp
    Timestamp(Timestamp),
    /// Sort by received time (fallback)
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
            // Timestamps come before received times
            (SortKey::Timestamp(_), SortKey::ReceivedAt(_)) => Ordering::Less,
            (SortKey::ReceivedAt(_), SortKey::Timestamp(_)) => Ordering::Greater,
        }
    }
}

// Wrapper for BinaryHeap (which is max-heap) to act as min-heap
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
        // Reverse the ordering for min-heap behavior
        other.0.sort_key().cmp(&self.0.sort_key())
    }
}

/// Configuration for the batch processor
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// How long to wait before emitting a partial batch
    pub batch_window: Duration,
    /// Maximum number of lines to hold in a batch
    pub max_batch_size: usize,
    /// Maximum memory usage for buffering (approximate)
    pub max_buffer_memory: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_window: Duration::from_millis(250),
            max_batch_size: 200,
            max_buffer_memory: 2 * 1024 * 1024, // 2MB
        }
    }
}

/// Batch processor that sorts lines by timestamp
pub struct BatchProcessor {
    /// Configuration for batching
    config: BatchConfig,
    /// Priority queue for sorting lines
    pending_lines: BinaryHeap<MinHeapLine>,
    /// Channel for receiving new lines
    receiver: Option<mpsc::UnboundedReceiver<BatchedLine>>,
    /// Channel for sending sorted batches
    sender: Option<mpsc::UnboundedSender<Vec<BatchedLine>>>,
    /// When the current batch window started
    window_start: Option<Instant>,
}

impl BatchProcessor {
    /// Create a new BatchProcessor
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            pending_lines: BinaryHeap::new(),
            receiver: None,
            sender: None,
            window_start: None,
        }
    }

    /// Start processing batches
    pub async fn start(
        &mut self,
    ) -> Result<(
        mpsc::UnboundedSender<BatchedLine>,
        mpsc::UnboundedReceiver<Vec<BatchedLine>>,
    )> {
        let (line_sender, line_receiver) = mpsc::unbounded_channel();
        let (batch_sender, batch_receiver) = mpsc::unbounded_channel();

        // Move the channels into a new processor instance for the spawned task
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

    /// Process incoming lines and emit sorted batches
    async fn process_loop(&mut self) -> Result<()> {
        let Some(line_receiver) = self.receiver.take() else {
            return Err(anyhow!("could not take a line receiver"));
        };
        let Some(batch_sender) = self.sender.take() else {
            return Err(anyhow!("could not take a batch sender from our sender"));
        };
        let mut line_receiver = line_receiver;

        // Set up timeout for batch windows
        let batch_timeout = tokio::time::interval(self.config.batch_window);
        tokio::pin!(batch_timeout);

        loop {
            tokio::select! {
                // New line received
                line_opt = line_receiver.recv() => {
                    match line_opt {
                        Some(mut line) => {
                            // Parse the line to extract timestamp
                            if let Err(_e) = line.parse() {
                                // this might fail, but we carry on
                                // eprintln!("Failed to parse line: {}", e);
                            }

                            // Add to pending lines heap
                            self.pending_lines.push(MinHeapLine(line));

                            // Start batch window if this is the first line
                            if self.window_start.is_none() {
                                self.window_start = Some(Instant::now());
                            }

                            // Check if we should emit due to size
                            if self.pending_lines.len() >= self.config.max_batch_size {
                                self.emit_batch(&batch_sender)?;
                            }
                        }
                        None => {
                            // Channel closed, emit any remaining lines and exit
                            self.emit_all_pending(&batch_sender)?;
                            break;
                        }
                    }
                }

                // Batch timeout elapsed
                _ = batch_timeout.tick() => {
                    if !self.pending_lines.is_empty() {
                        self.emit_batch(&batch_sender)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Emit the current batch
    fn emit_batch(&mut self, batch_sender: &mpsc::UnboundedSender<Vec<BatchedLine>>) -> Result<()> {
        if self.pending_lines.is_empty() {
            return Ok(());
        }

        // Convert heap to sorted vector (min-heap gives us sorted order)
        let mut sorted_lines = Vec::new();
        while let Some(min_line) = self.pending_lines.pop() {
            sorted_lines.push(min_line.0);
        }

        // Reset batch window
        self.window_start = None;

        // Send the sorted batch
        if batch_sender.send(sorted_lines).is_err() {
            // Receiver dropped; return error to stop processing
            return Err(anyhow!("Batch receiver dropped"));
        }

        Ok(())
    }

    /// Force emit all pending lines as a batch
    fn emit_all_pending(&mut self, batch_sender: &mpsc::UnboundedSender<Vec<BatchedLine>>) -> Result<()> {
        self.emit_batch(batch_sender)
    }
}

/// Create a new batch processor with default configuration
pub fn create_processor() -> BatchProcessor {
    BatchProcessor::new(BatchConfig::default())
}

/// Create a new batch processor with custom configuration
pub fn create_processor_with_config(config: BatchConfig) -> BatchProcessor {
    BatchProcessor::new(config)
}
