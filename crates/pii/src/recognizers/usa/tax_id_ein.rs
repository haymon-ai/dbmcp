//! `TAX_ID_EIN` recognizer (US Employer Identification Number).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `TAX_ID_EIN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_id_ein_usa() -> Recognizer {
    let pattern =
        Pattern::new("US EIN", r"\b\d{2}-\d{7}\b", Score::from_static(0.5)).expect("static EIN pattern compiles");
    Recognizer::new(Entity::TaxIdEin, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("TaxIdEinUsaRecognizer")
        .with_validator(Validator::EinPrefixUsa)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::tax_id_ein_usa;

    fn results(text: &str) -> Vec<(&str, f32)> {
        tax_id_ein_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_tax_id_ein_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("EIN 04-1234567", &[("04-1234567", 1.0)]),
            ("07-1234567", &[]),
            ("08-1234567", &[]),
            ("09-1234567", &[]),
            ("17-1234567", &[]),
            ("18-1234567", &[]),
            ("19-1234567", &[]),
            ("28-1234567", &[]),
            ("29-1234567", &[]),
            ("49-1234567", &[]),
            ("69-1234567", &[]),
            ("70-1234567", &[]),
            ("78-1234567", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
