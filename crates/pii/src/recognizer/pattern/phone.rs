//! `PHONE_NUMBER` recognizer.
//!
//! Regex-only — E.164 plus region-targeted patterns for `US`, `UK`, `DE`,
//! all at score `0.4`. `libphonenumber`-grade parse/validation deferred.

use crate::recognizer::{Category, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `PHONE_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn phone_number() -> Pattern {
    let s = Score::from_static(0.4);
    let patterns = vec![
        Regex::new("E.164", r"\+\d{8,15}\b", s).expect("E.164 compiles"),
        Regex::new("US", r"\b(?:\+?1[\s-]?)?\(?\d{3}\)?[\s-]?\d{3}[\s-]?\d{4}\b", s).expect("US compiles"),
        Regex::new("UK", r"\b(?:\+?44[\s-]?|0)(?:\d[\s-]?){9,10}\b", s).expect("UK compiles"),
        Regex::new("DE", r"\b(?:\+?49[\s-]?|0)(?:\d[\s-]?){6,12}\b", s).expect("DE compiles"),
    ];
    Pattern::new(entity::PHONE_NUMBER, patterns)
        .expect("non-empty pattern list")
        .with_name("PhoneRecognizer")
        .with_category(Category::Contact)
}
