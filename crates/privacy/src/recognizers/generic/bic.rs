//! `BIC_CODE` recognizer (ISO 9362 SWIFT/BIC) with country-code validator.

use crate::recognizers::prelude::*;

/// Context keywords for BIC / SWIFT codes.
///
/// A bare `swift` is left out: it also names the programming language
/// (`swift_version`) and fronts status words (`SWIFT: REQUIRED`) that fit the
/// BIC shape.
const CONTEXT: &[&str] = &[
    "bic",
    "swift code",
    "swiftcode",
    "iso 9362",
    "bank identifier code",
    "code swift",
];

/// Build the `BIC_CODE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn bic_code() -> Recognizer {
    let pattern = Pattern::new(
        "BIC (8 or 11)",
        r"\b[A-Z]{6}[A-Z0-9]{2}(?:[A-Z0-9]{3})?\b",
        Score::from_static(0.4),
    )
    .expect("static BIC pattern compiles")
    .requiring_context();
    Recognizer::new(Entity::BicCode, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("BicCodeRecognizer")
        .with_validator(Validator::BicCountry)
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, bic_code};

    #[test]
    fn carries_context_list() {
        assert_eq!(bic_code().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        bic_code()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_bic_code() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("bic DEUTDEFF", &[("DEUTDEFF", 1.0)]),
            ("bic DEUTDEFF500", &[("DEUTDEFF500", 1.0)]),
            (
                "DEUTDEFF500 and COBADEFFXXX",
                &[("DEUTDEFF500", 1.0), ("COBADEFFXXX", 1.0)],
            ),
            ("bic DEUTXXFF", &[]),
            ("bic deutdeff500", &[]),
            ("bic DEUTDEFF5", &[]),
            ("bic DEUTDEFF50", &[]),
            ("bic DEUTDEFF5000", &[]),
            ("bic DEUT DE FF", &[]),
            ("bic DEU1DEFF", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
