//! `POSTCODE_DE` recognizer (Postleitzahl / PLZ, keyword context required).
//!
//! Gated by Presidio's `DePlzRecognizer` keywords except `adresse`, which is
//! French too and so gates the country-neutral `POSTCODE`, as do generic words
//! such as `zip`. Registered before `POSTCODE`, so it wins a tie when keywords
//! of both kinds support one value.

use crate::recognizers::prelude::*;

/// Context keywords for DE Postleitzahl.
const CONTEXT: &[&str] = &[
    "plz",
    "postleitzahl",
    "postanschrift",
    "wohnort",
    "ort",
    "wohnanschrift",
    "lieferadresse",
    "rechnungsadresse",
    "straße",
    "strasse",
    "hausnummer",
    "postfach",
    "bundesland",
    "gemeinde",
    "stadt",
    "dorf",
];

/// Build the `POSTCODE_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn postcode_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Postcode",
        r"\b(?!01000\b|99999\b)(0[1-9]\d{3}|[1-9]\d{4})\b",
        Score::from_static(0.05),
    )
    .expect("static DE postcode pattern compiles")
    .requiring_context();
    Recognizer::new(Entity::PostcodeDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PostcodeDeuRecognizer")
        .with_category(Category::Contact)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, postcode_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(postcode_deu().context(), CONTEXT);
    }

    #[test]
    fn requires_context() {
        assert!(
            postcode_deu()
                .patterns()
                .iter()
                .all(crate::pattern::Pattern::requires_context)
        );
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        postcode_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_postcode_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("10115", &[("10115", 0.05)]),
            ("80331", &[("80331", 0.05)]),
            ("22085", &[("22085", 0.05)]),
            ("01001", &[("01001", 0.05)]),
            ("99998", &[("99998", 0.05)]),
            ("PLZ: 10115", &[("10115", 0.05)]),
            ("Postleitzahl 80331 München", &[("80331", 0.05)]),
            ("00000", &[]),
            ("01000", &[]),
            ("99999", &[]),
            ("101150", &[]),
            ("1011", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
