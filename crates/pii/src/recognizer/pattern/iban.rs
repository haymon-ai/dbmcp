//! `IBAN_CODE` recognizer with mod-97 validator.

use crate::recognizer::{Category, IbanValidator, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `IBAN_CODE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn iban() -> Pattern {
    let pattern = Regex::new(
        "IBAN (generic)",
        r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b",
        Score::from_static(0.5),
    )
    .expect("static IBAN pattern compiles");
    Pattern::new(entity::IBAN_CODE, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("IbanRecognizer")
        .with_validator(IbanValidator)
        .with_category(Category::Financial)
}
