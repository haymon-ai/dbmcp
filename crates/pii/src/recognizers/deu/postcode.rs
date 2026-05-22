//! `POSTCODE_DE` recognizer (Postleitzahl / PLZ, weak 0.05 base score — requires context).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for DE Postleitzahl.
const CONTEXT: &[&str] = &[
    "plz",
    "postleitzahl",
    "postanschrift",
    "adresse",
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
    "zip",
    "postcode",
    "postal",
    "address",
    "city",
    "delivery",
    "mailing",
    "shipping",
    "correspondence",
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
    .expect("static DE postcode pattern compiles");
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
