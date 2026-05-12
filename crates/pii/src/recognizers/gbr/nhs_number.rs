//! `NHS_NUMBER` recognizer (UK NHS patient identifier with mod-11 checksum).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for UK NHS number.
const CONTEXT: &[&str] = &[
    "national health service",
    "nhs",
    "health services authority",
    "health authority",
];

/// Build the `NHS_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn nhs_number_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK NHS number",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{4}\b",
        Score::from_static(0.4),
    )
    .expect("static NHS pattern compiles");
    Recognizer::new(Entity::NhsNumber, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("NhsNumberGbrRecognizer")
        .with_validator(Validator::Mod11NhsGbr)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::nhs_number_gbr;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        nhs_number_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_nhs_number_gbr() {
        // 943 476 5919 — valid NHS test number; 0000000051 — remainder-10 branch
        // (sum%11 == 10 → check digit 1) regression case.
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("401-023-2137", &[(0, 12)]),
            ("221 395 1837", &[(0, 12)]),
            ("0032698674", &[(0, 10)]),
            ("NHS 943 476 5919", &[(4, 16)]),
            ("NHS 0000000051", &[(4, 14)]),
            ("401-023-2138", &[]),
            ("NHS 943 476 5910", &[]),
            ("NHS 943 476 5917", &[]),
            ("123456789", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
