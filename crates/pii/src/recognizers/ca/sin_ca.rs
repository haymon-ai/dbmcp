//! `SIN_CA` recognizer (Luhn-validated, keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const KEYWORDS: &[&str] = &[
    "sin",
    "social insurance",
    "numéro d'assurance sociale",
    "assurance sociale",
];

/// Build the `SIN_CA` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source, score literal, or keyword set is rejected at construction.
#[must_use]
pub fn sin_ca() -> Recognizer {
    let pattern = Pattern::new(
        "Canadian SIN",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{3}\b",
        Score::from_static(0.4),
    )
    .expect("static SIN_CA pattern compiles");
    let validator = Validator::And(
        Box::new(Validator::LuhnSin),
        Box::new(Validator::Keyword(KeywordValidator::new(KEYWORDS))),
    );
    Recognizer::new(Entity::SinCa, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SinCaRecognizer")
        .with_validator(validator)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::sin_ca;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        sin_ca().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_sin_ca() {
        // 046 454 286 — known-valid Canadian SIN test number.
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("SIN 046 454 286", &[(4, 15)]),
            ("social insurance 046-454-286", &[(17, 28)]),
            ("sin: 046454286", &[(5, 14)]),
            ("046 454 286", &[]),
            ("SIN 046 454 287", &[]),
            ("SIN 146 454 286", &[]),
            ("SIN 12345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
