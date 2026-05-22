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
    use super::{CONTEXT, iban};

    #[test]
    fn carries_context_list() {
        assert_eq!(iban().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        iban()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_iban() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("DE89370400440532013000", &[("DE89370400440532013000", 1.0)]),
            ("GB82WEST12345698765432", &[("GB82WEST12345698765432", 1.0)]),
            ("FR1420041010050500013M02606", &[("FR1420041010050500013M02606", 1.0)]),
            ("BE62510007547061", &[("BE62510007547061", 1.0)]),
            (
                "transfer to DE89370400440532013000 today",
                &[("DE89370400440532013000", 1.0)],
            ),
            (
                "DE89370400440532013000 GB82WEST12345698765432",
                &[("DE89370400440532013000", 1.0), ("GB82WEST12345698765432", 1.0)],
            ),
            ("DE00370400440532013000", &[]),
            ("DE89 3704 0044 0532 0130 00", &[]),
            ("de89370400440532013000", &[]),
            ("DE8937", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
