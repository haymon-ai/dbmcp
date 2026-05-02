//! `CREDIT_CARD` recognizer with Luhn checksum validator.
//!
//! Pattern adapted from Presidio's `CreditCardRecognizer.PATTERNS["All Credit Cards (weak)"]`.

use crate::pattern::Pattern;
use crate::recognizer::{LuhnValidator, PatternRecognizer, entity};
use crate::score::Score;

/// Build the `CREDIT_CARD` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn credit_card() -> PatternRecognizer {
    let pattern = Pattern::new(
        "All Credit Cards (weak)",
        r"\b((4\d{3})|(5[0-5]\d{2})|(6\d{3})|(1\d{3})|(3\d{3}))[- ]?(\d{3,4})[- ]?(\d{3,4})[- ]?(\d{3,5})\b",
        Score::from_static(0.3),
    )
    .expect("static credit-card pattern compiles");
    PatternRecognizer::new(entity::CREDIT_CARD, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("CreditCardRecognizer")
        .with_validator(LuhnValidator)
}
