//! `US_SSN` recognizer.
//!
//! Plain regex matches `XXX-XX-XXXX` shape; reserved area/group/serial values
//! are rejected by [`UsSsnValidator`] (replaces Presidio's negative-lookahead
//! constructs).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `US_SSN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn us_ssn() -> Recognizer {
    let pattern = Pattern::new("US SSN", r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b", Score::from_static(0.6))
        .expect("static SSN pattern compiles");
    Recognizer::new(Entity::UsSsn, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("UsSsnRecognizer")
        .with_validator(Validator::UsSsn)
        .with_category(Category::Government)
}
