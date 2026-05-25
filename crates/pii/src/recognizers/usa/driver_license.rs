//! `DRIVER_LICENSE_US` recognizer.
//!
//! Two patterns: a 23-alternation alphanumeric shape covering documented
//! per-state formats (score `0.3`) and a digit-only shape for states whose
//! licence is purely numeric (score `0.01`). Both are gated by a keyword
//! context validator because the regex alone matches too many false positives.

use crate::recognizers::prelude::*;

/// Context keywords for US driver licence.
const CONTEXT: &[&str] = &[
    "driver",
    "license",
    "permit",
    "lic",
    "identification",
    "dls",
    "cdls",
    "driving",
];

const ALPHANUMERIC_PATTERN: &str = concat!(
    r"\b(",
    r"[A-Z][0-9]{3,6}",
    r"|[A-Z][0-9]{5,9}",
    r"|[A-Z][0-9]{6,8}",
    r"|[A-Z][0-9]{4,8}",
    r"|[A-Z][0-9]{9,11}",
    r"|[A-Z]{1,2}[0-9]{5,6}",
    r"|H[0-9]{8}",
    r"|V[0-9]{6}",
    r"|X[0-9]{8}",
    r"|[A-Z]{2}[0-9]{2,5}",
    r"|[A-Z]{2}[0-9]{3,7}",
    r"|[0-9]{2}[A-Z]{3}[0-9]{5,6}",
    r"|[A-Z][0-9]{13,14}",
    r"|[A-Z][0-9]{18}",
    r"|[A-Z][0-9]{6}R",
    r"|[A-Z][0-9]{9}",
    r"|[A-Z][0-9]{1,12}",
    r"|[0-9]{9}[A-Z]",
    r"|[A-Z]{2}[0-9]{6}[A-Z]",
    r"|[0-9]{8}[A-Z]{2}",
    r"|[0-9]{3}[A-Z]{2}[0-9]{4}",
    r"|[A-Z][0-9][A-Z][0-9][A-Z]",
    r"|[0-9]{7,8}[A-Z]",
    r")\b",
);

const DIGIT_PATTERN: &str = r"\b(?:[0-9]{6,14}|[0-9]{16})\b";

/// Build the `DRIVER_LICENSE_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex sources or score literals are rejected at construction.
#[must_use]
pub fn driver_license_usa() -> Recognizer {
    let alphanumeric = Pattern::new(
        "US Driver License - Alphanumeric",
        ALPHANUMERIC_PATTERN,
        Score::from_static(0.3),
    )
    .expect("static DL alphanumeric pattern compiles");
    let digits = Pattern::new("US Driver License - Digits", DIGIT_PATTERN, Score::from_static(0.01))
        .expect("static DL digit pattern compiles");
    Recognizer::new(Entity::DriverLicenseUs, vec![alphanumeric, digits])
        .expect("non-empty pattern list")
        .with_name("DriverLicenseUsaRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, driver_license_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(driver_license_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        driver_license_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_driver_license_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("driver license A1234567", &[("A1234567", 0.3)]),
            ("DL: H12345678", &[("H12345678", 0.3)]),
            ("driving permit 1234567", &[("1234567", 0.01)]),
            ("cdl 12345678901234", &[("12345678901234", 0.01)]),
            ("driving licence 1234567890123456", &[("1234567890123456", 0.01)]),
            ("order A1234567", &[("A1234567", 0.3)]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
