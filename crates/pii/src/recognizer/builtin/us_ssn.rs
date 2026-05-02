//! `US_SSN` recognizer.
//!
//! Plain regex matches `XXX-XX-XXXX` shape; reserved area/group/serial values
//! are rejected by [`UsSsnValidator`] (replaces Presidio's negative-lookahead
//! constructs).

use crate::pattern::Pattern;
use crate::recognizer::{PatternRecognizer, UsSsnValidator, entity};
use crate::score::Score;

/// Build the `US_SSN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn us_ssn() -> PatternRecognizer {
    let pattern = Pattern::new("US SSN", r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b", Score::from_static(0.6))
        .expect("static SSN pattern compiles");
    PatternRecognizer::new(entity::US_SSN, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("UsSsnRecognizer")
        .with_validator(UsSsnValidator)
}
