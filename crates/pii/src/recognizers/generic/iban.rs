//! `IBAN_CODE` recognizer with mod-97 validator.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for IBAN.
const CONTEXT: &[&str] = &["iban", "bank", "transaction"];

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
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::iban;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        iban().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_iban() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("DE89370400440532013000", &[(0, 22)]),
            ("GB82WEST12345698765432", &[(0, 22)]),
            ("FR1420041010050500013M02606", &[(0, 27)]),
            ("BE62510007547061", &[(0, 16)]),
            ("transfer to DE89370400440532013000 today", &[(12, 34)]),
            ("DE89370400440532013000 GB82WEST12345698765432", &[(0, 22), (23, 45)]),
            ("DE00370400440532013000", &[]),
            ("DE89 3704 0044 0532 0130 00", &[]),
            ("de89370400440532013000", &[]),
            ("DE8937", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
