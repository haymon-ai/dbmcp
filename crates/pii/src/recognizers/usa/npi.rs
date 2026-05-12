//! `NPI_US` recognizer (National Provider Identifier).
//!
//! Ten-digit identifier (with optional space/dash separators every 3 digits
//! after the leading entity-type digit). Validated by the CMS NPI Luhn
//! algorithm — `"80840"` prefix prepended before the standard Luhn pass —
//! with an additional filter rejecting all-identical-body numbers.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for US NPI.
const CONTEXT: &[&str] = &[
    "npi",
    "national provider",
    "provider",
    "npi number",
    "provider id",
    "provider identifier",
    "taxonomy",
];

/// Build the `NPI_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex sources or score literals are rejected at construction.
#[must_use]
pub fn npi_usa() -> Recognizer {
    let bare =
        Pattern::new("US NPI", r"\b[12]\d{9}\b", Score::from_static(0.1)).expect("static NPI bare pattern compiles");
    let dashed = Pattern::new(
        "US NPI (separated)",
        r"\b[12]\d{3}[ -]\d{3}[ -]\d{3}\b",
        Score::from_static(0.4),
    )
    .expect("static NPI separated pattern compiles");
    Recognizer::new(Entity::NpiUs, vec![bare, dashed])
        .expect("non-empty pattern list")
        .with_name("NpiUsaRecognizer")
        .with_validator(Validator::NpiUsa)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::npi_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        npi_usa().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_npi_usa() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("NPI 1234567893", &[(4, 14)]),
            ("provider 1234-567-893", &[(9, 21)]),
            ("npi 1234 567 893", &[(4, 16)]),
            ("NPI 1234567890", &[]),
            ("NPI 3234567893", &[]),
            ("NPI 9999999995", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
