//! `VAT_NUMBER` recognizer (EU / UK / Northern Ireland VAT identifier).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `VAT_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn vat_number() -> Recognizer {
    let pattern = Pattern::new(
        "VAT (ISO2 + body)",
        r"\b[A-Z]{2}[A-Z0-9]{7,12}\b",
        Score::from_static(0.4),
    )
    .expect("static VAT pattern compiles");
    Recognizer::new(Entity::VatNumber, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("VatNumberRecognizer")
        .with_validator(Validator::VatCountryLength)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::vat_number;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        vat_number()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_vat_number() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("VAT DE123456789", &[(4, 15)]),
            ("VAT GB123456789", &[(4, 15)]),
            ("billing DE123456789 and GB987654321", &[(8, 19), (24, 35)]),
            ("VAT XX123456789", &[]),
            ("DE12345", &[]),
            ("VAT de123456789", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
