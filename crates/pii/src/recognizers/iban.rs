//! `IBAN_CODE` recognizer with mod-97 validator.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `IBAN_CODE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn iban() -> Recognizer {
    let pattern = Pattern::new(
        "IBAN (generic)",
        r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b",
        Score::from_static(0.5),
    )
    .expect("static IBAN pattern compiles");
    Recognizer::new(Entity::IbanCode, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("IbanRecognizer")
        .with_validator(Validator::Iban)
        .with_category(Category::Financial)
}
