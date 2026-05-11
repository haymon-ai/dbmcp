//! Shared helpers for the PII benches: payload synthesis on top of the
//! `Corpus` loader.

#![allow(dead_code)]

#[path = "corpus.rs"]
mod corpus;

use corpus::Corpus;
use dbmcp_pii::{AnalyzeOptions, Analyzer, RecognizerResult};

/// Input sizes (bytes) swept by the throughput benches.
pub const SIZES: &[usize] = &[1024, 8 * 1024, 64 * 1024, 512 * 1024];

const FILLER: &str = "the quick brown fox jumps over the lazy dog while logs ship and metrics tick along the wire ";

/// Build a deterministic payload of approximately `size_bytes` bytes by
/// interleaving filler prose with corpus positives modulo the input slice.
#[must_use]
pub fn synth_payload(size_bytes: usize, positives: &[String]) -> String {
    assert!(!positives.is_empty(), "positives must not be empty");
    let mut out = String::with_capacity(size_bytes + 256);
    let mut cycle = positives.iter().cycle();
    while out.len() < size_bytes {
        out.push_str(FILLER);
        out.push_str(cycle.next().expect("positives non-empty"));
        out.push(' ');
    }
    out
}

/// Build a mixed payload using positives from several corpora.
#[must_use]
pub fn mixed_payload(size_bytes: usize) -> String {
    synth_payload(size_bytes, &pii_pool(&["email", "credit_card", "iban", "ip", "url"]))
}

/// Concatenate positives from each corpus stem into a single pool.
#[must_use]
pub fn pii_pool(stems: &[&str]) -> Vec<String> {
    stems.iter().flat_map(|s| Corpus::load(s).positives).collect()
}

/// Pre-compute analyzer results for `text` so anonymizer benches don't pay
/// recognition cost on the hot path.
#[must_use]
pub fn sample_results(analyzer: &Analyzer, text: &str) -> Vec<RecognizerResult> {
    analyzer.analyze(text, &AnalyzeOptions::default())
}
