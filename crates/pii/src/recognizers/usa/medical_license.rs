//! `MEDICAL_LICENSE_US` recognizer (DEA Certificate Number).
//!
//! Two-letter prefix followed by seven digits; the trailing digit is a
//! Luhn-variant check digit derived from the preceding six. Match scored
//! `0.4` and gated by keyword context to suppress false positives on
//! arbitrary alphanumeric tokens.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for US medical license.
const CONTEXT: &[&str] = &["medical", "certificate", "dea"];

const PATTERN: &str = concat!(
    r"\b[abcdefghjklmprstuxABCDEFGHJKLMPRSTUX][a-zA-Z]\d{7}\b",
    r"|",
    r"\b[abcdefghjklmprstuxABCDEFGHJKLMPRSTUX]9\d{7}\b",
);

/// Build the `MEDICAL_LICENSE_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn medical_license_usa() -> Recognizer {
    let pattern = Pattern::new("US DEA", PATTERN, Score::from_static(0.4)).expect("static DEA pattern compiles");
    Recognizer::new(Entity::MedicalLicenseUs, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("MedicalLicenseUsaRecognizer")
        .with_validator(Validator::MedicalLicenseUsaDea)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, medical_license_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(medical_license_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        medical_license_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_medical_license_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("DEA #: AB1234563", &[("AB1234563", 1.0)]),
            ("dea AB1234563", &[("AB1234563", 1.0)]),
            ("medical certificate A91234563", &[("A91234563", 1.0)]),
            ("random AB1234563", &[("AB1234563", 1.0)]),
            ("DEA AB1234560", &[]),
            ("DEA IB1234563", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
