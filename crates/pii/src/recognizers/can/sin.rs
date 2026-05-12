//! `SIN_CA` recognizer (Luhn-validated, keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for Canadian SIN.
const CONTEXT: &[&str] = &[
    "sin",
    "sin number",
    "social insurance",
    "social insurance number",
    "canada",
    "nas",
    "numéro nas",
    "numéro d'assurance sociale",
    "assurance sociale",
];

/// Build the `SIN_CA` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source, score literal, or keyword set is rejected at construction.
#[must_use]
pub fn sin_can() -> Recognizer {
    let pattern = Pattern::new(
        "Canadian SIN",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{3}\b",
        Score::from_static(0.4),
    )
    .expect("static SIN_CA pattern compiles");
    Recognizer::new(Entity::SinCa, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SinCanRecognizer")
        .with_validator(Validator::LuhnSinCan)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::sin_can;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        sin_can().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_sin_can() {
        // 046 454 286 — known-valid Canadian SIN test number.
        // Recognizer-level results: Luhn-validator passes → MAX score.
        // Keyword gating is handled by the context-boost pass + redactor
        // `min_score` floor, not by the recognizer itself.
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("SIN 046 454 286", &[(4, 15)]),
            ("social insurance 046-454-286", &[(17, 28)]),
            ("sin: 046454286", &[(5, 14)]),
            ("046 454 286", &[(0, 11)]),
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
