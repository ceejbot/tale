//! JSON deserialization profiling
//!
//! This module provides tools to profile JSON parsing performance and
//! measure which Printable variants are hit most frequently.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::logpatterns::Printable;

/// Global counters for tracking Printable variant usage
#[derive(Debug, Default)]
pub struct VariantCounters {
    pub canonical: AtomicUsize,
    pub java: AtomicUsize,
    pub message: AtomicUsize,
    pub time_only: AtomicUsize,
    pub json: AtomicUsize,
    pub logfmt: AtomicUsize,
    pub text: AtomicUsize,
    pub parse_errors: AtomicUsize,
}

static COUNTERS: LazyLock<VariantCounters> = LazyLock::new(Default::default);

impl VariantCounters {
    /// Record that we successfully parsed a specific variant
    pub fn record_variant(&self, variant: &Printable<'_>) {
        match variant {
            Printable::Canonical(_) => self.canonical.fetch_add(1, Ordering::Relaxed),
            Printable::Java(_) => self.java.fetch_add(1, Ordering::Relaxed),
            Printable::Message(_) => self.message.fetch_add(1, Ordering::Relaxed),
            Printable::TimeOnly(_) => self.time_only.fetch_add(1, Ordering::Relaxed),
            Printable::Json(_) => self.json.fetch_add(1, Ordering::Relaxed),
            Printable::Logfmt(_) => self.logfmt.fetch_add(1, Ordering::Relaxed),
            Printable::Text(_) => self.text.fetch_add(1, Ordering::Relaxed),
        };
    }

    /// Record that JSON parsing failed entirely
    pub fn record_parse_error(&self) {
        self.parse_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current counts for all variants
    pub fn get_counts(&self) -> VariantCounts {
        VariantCounts {
            canonical: self.canonical.load(Ordering::Relaxed),
            java: self.java.load(Ordering::Relaxed),
            message: self.message.load(Ordering::Relaxed),
            time_only: self.time_only.load(Ordering::Relaxed),
            json: self.json.load(Ordering::Relaxed),
            logfmt: self.logfmt.load(Ordering::Relaxed),
            text: self.text.load(Ordering::Relaxed),
            parse_errors: self.parse_errors.load(Ordering::Relaxed),
        }
    }

    /// Reset all counters to zero
    pub fn reset(&self) {
        self.canonical.store(0, Ordering::Relaxed);
        self.java.store(0, Ordering::Relaxed);
        self.message.store(0, Ordering::Relaxed);
        self.time_only.store(0, Ordering::Relaxed);
        self.json.store(0, Ordering::Relaxed);
        self.logfmt.store(0, Ordering::Relaxed);
        self.text.store(0, Ordering::Relaxed);
        self.parse_errors.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of variant counts at a point in time
#[derive(Debug, Clone)]
pub struct VariantCounts {
    pub canonical: usize,
    pub java: usize,
    pub message: usize,
    pub time_only: usize,
    pub json: usize,
    pub logfmt: usize,
    pub text: usize,
    pub parse_errors: usize,
}

impl VariantCounts {
    pub fn total(&self) -> usize {
        self.canonical + self.java + self.message + self.time_only + self.json + self.logfmt + self.text
    }

    pub fn successful_parses(&self) -> usize {
        self.total() - self.text // Text is plain text, not JSON
    }

    pub fn json_parses(&self) -> usize {
        self.canonical + self.java + self.message + self.time_only + self.json
    }

    /// Calculate percentage of each variant
    pub fn percentages(&self) -> VariantPercentages {
        let total = self.total() as f64;
        if total == 0.0 {
            return VariantPercentages::default();
        }

        VariantPercentages {
            canonical: (self.canonical as f64 / total) * 100.0,
            java: (self.java as f64 / total) * 100.0,
            message: (self.message as f64 / total) * 100.0,
            time_only: (self.time_only as f64 / total) * 100.0,
            json: (self.json as f64 / total) * 100.0,
            logfmt: (self.logfmt as f64 / total) * 100.0,
            text: (self.text as f64 / total) * 100.0,
            parse_errors: (self.parse_errors as f64 / total) * 100.0,
        }
    }

    /// Print a formatted report of variant usage
    pub fn print_report(&self) {
        let percentages = self.percentages();
        let total = self.total();

        println!("JSON Parsing Profile Report:");
        println!("============================");
        println!("Total lines processed: {}", total);
        println!(
            "JSON lines: {} ({:.1}%)",
            self.json_parses(),
            (self.json_parses() as f64 / total as f64) * 100.0
        );
        println!("Plain text: {} ({:.1}%)", self.text, percentages.text);
        println!("Parse errors: {} ({:.1}%)", self.parse_errors, percentages.parse_errors);
        println!();
        println!("JSON Variant Breakdown:");
        println!(
            "  Canonical: {} ({:.1}%) - FASTEST",
            self.canonical, percentages.canonical
        );
        println!("  Java:      {} ({:.1}%)", self.java, percentages.java);
        println!(
            "  Message:   {} ({:.1}%) - SUPERSET (includes GCP, Logstash, etc.)",
            self.message, percentages.message
        );
        println!("  TimeOnly:  {} ({:.1}%)", self.time_only, percentages.time_only);
        println!("  Generic:   {} ({:.1}%) - FALLBACK", self.json, percentages.json);
        println!("  Logfmt:    {} ({:.1}%)", self.logfmt, percentages.logfmt);
        println!();

        if percentages.canonical > 50.0 {
            println!(
                "✅ Good: {}% of logs use the fast Canonical path",
                percentages.canonical
            );
        } else if percentages.canonical > 25.0 {
            println!(
                "⚠️  Moderate: {}% of logs use the fast Canonical path",
                percentages.canonical
            );
        } else {
            println!(
                "❌ Poor: Only {}% of logs use the fast Canonical path",
                percentages.canonical
            );
            println!("   Consider adding more fields to Canonical or optimizing Message parsing");
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VariantPercentages {
    pub canonical: f64,
    pub java: f64,
    pub message: f64,
    pub time_only: f64,
    pub json: f64,
    pub logfmt: f64,
    pub text: f64,
    pub parse_errors: f64,
}

/// Global profiling interface
pub fn record_variant(variant: &Printable<'_>) {
    COUNTERS.record_variant(variant);
}

pub fn record_parse_error() {
    COUNTERS.record_parse_error();
}

pub fn get_counts() -> VariantCounts {
    COUNTERS.get_counts()
}

pub fn reset_counters() {
    COUNTERS.reset();
}

pub fn print_report() {
    get_counts().print_report();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_count_variants() {
        reset_counters();

        // This would require creating actual Printable instances
        // For now, just test the percentage calculation
        let counts = VariantCounts {
            canonical: 50,
            message: 30,
            json: 15,
            text: 5,
            java: 0,
            logfmt: 0,
            time_only: 0,
            parse_errors: 0,
        };

        let percentages = counts.percentages();
        assert_eq!(counts.total(), 100);
        assert!((percentages.canonical - 50.0).abs() < 0.1);
        assert!((percentages.message - 30.0).abs() < 0.1);
        assert!((percentages.json - 15.0).abs() < 0.1);
        assert!((percentages.text - 5.0).abs() < 0.1);
    }
}
