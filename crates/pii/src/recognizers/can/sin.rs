//! `SIN_CA` recognizer (Luhn-validated, keyword-context required).

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, sin_can};

    #[test]
    fn carries_context_list() {
        assert_eq!(sin_can().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        sin_can()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_sin_can() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("SIN 046 454 286", &[("046 454 286", 1.0)]),
            ("social insurance 046-454-286", &[("046-454-286", 1.0)]),
            ("sin: 046454286", &[("046454286", 1.0)]),
            ("046 454 286", &[("046 454 286", 1.0)]),
            ("SIN 046 454 287", &[]),
            ("SIN 146 454 286", &[]),
            ("SIN 12345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
