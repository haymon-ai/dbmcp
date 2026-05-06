//! Keyword-context validator.
//!
//! Used by recognizers whose regex is too weak to stand alone (e.g. `CVV`
//! matching `\d{3,4}`). Returns `Valid` only when one of a configured keyword
//! set appears within ±N characters of the candidate span; otherwise returns
//! `Invalid` (drop the match).

use std::ops::Range;

use crate::recognizer::{ValidationOutcome, Validator};

/// Default character window either side of the candidate span.
pub const DEFAULT_WINDOW: usize = 64;

/// Validator that requires at least one configured keyword within `±window`
/// characters of the candidate span.
///
/// Internally compiles the keywords into a single `(?i-u)` regex alternation,
/// which the `regex` crate optimises into an Aho-Corasick automaton with
/// SIMD-accelerated literal scanning. The hot path is one `is_match` call —
/// no per-call allocation, no per-keyword lowercasing.
pub struct KeywordValidator {
    matcher: regex::Regex,
    window: usize,
}

impl std::fmt::Debug for KeywordValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeywordValidator")
            .field("pattern", &self.matcher.as_str())
            .field("window", &self.window)
            .finish()
    }
}

impl KeywordValidator {
    /// Build a validator with the default `±64`-character window.
    ///
    /// # Panics
    ///
    /// Panics if `keywords` is empty — a zero-keyword validator would accept
    /// every candidate and is always a configuration mistake.
    #[must_use]
    pub fn new(keywords: &'static [&'static str]) -> Self {
        assert!(!keywords.is_empty(), "KeywordValidator requires at least one keyword");
        let alternation = keywords
            .iter()
            .copied()
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join("|");
        // `(?i-u)` enables ASCII-only case-insensitive matching; faster than the
        // Unicode-aware default and sufficient for the lowercase keyword sets used here.
        let pattern = format!("(?i-u)({alternation})");
        let matcher = regex::Regex::new(&pattern).expect("escaped keyword alternation always compiles");
        Self {
            matcher,
            window: DEFAULT_WINDOW,
        }
    }

    /// Override the window size (characters either side).
    #[must_use]
    pub fn with_window(mut self, chars: usize) -> Self {
        self.window = chars;
        self
    }
}

impl Validator for KeywordValidator {
    fn validate(&self, _candidate: &str) -> ValidationOutcome {
        // No surrounding text available — strict mode rejects.
        ValidationOutcome::Invalid
    }

    fn validate_with_context(&self, _candidate: &str, full_text: &str, span: Range<usize>) -> ValidationOutcome {
        if !full_text.is_char_boundary(span.start) || !full_text.is_char_boundary(span.end) {
            return ValidationOutcome::Invalid;
        }
        let lo = snap_lo(full_text, span.start.saturating_sub(self.window));
        let hi = snap_hi(full_text, span.end.saturating_add(self.window).min(full_text.len()));
        if self.matcher.is_match(&full_text[lo..hi]) {
            ValidationOutcome::Valid
        } else {
            ValidationOutcome::Invalid
        }
    }
}

fn snap_lo(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn snap_hi(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYWORDS: &[&str] = &["cvv", "cvc"];

    #[test]
    fn invalid_without_context() {
        let v = KeywordValidator::new(KEYWORDS);
        assert_eq!(v.validate("123"), ValidationOutcome::Invalid);
    }

    #[test]
    fn valid_with_keyword_before() {
        let v = KeywordValidator::new(KEYWORDS);
        let text = "cvv: 123";
        let span = 5..8; // "123"
        assert_eq!(v.validate_with_context("123", text, span), ValidationOutcome::Valid);
    }

    #[test]
    fn valid_with_keyword_after() {
        let v = KeywordValidator::new(KEYWORDS);
        let text = "code 123 (cvc)";
        let span = 5..8;
        assert_eq!(v.validate_with_context("123", text, span), ValidationOutcome::Valid);
    }

    #[test]
    fn invalid_outside_window() {
        let v = KeywordValidator::new(KEYWORDS).with_window(2);
        let text = "cvv:                 123";
        let span = 21..24;
        assert_eq!(v.validate_with_context("123", text, span), ValidationOutcome::Invalid);
    }

    #[test]
    fn case_insensitive_keyword() {
        let v = KeywordValidator::new(KEYWORDS);
        let text = "CVV: 123";
        let span = 5..8;
        assert_eq!(v.validate_with_context("123", text, span), ValidationOutcome::Valid);
    }
}
