//! `TAX_ID_DE` recognizer (Steueridentifikationsnummer, ISO 7064 Mod 11, 10).

use crate::recognizers::prelude::*;

/// Context keywords for DE Steueridentifikationsnummer.
const CONTEXT: &[&str] = &[
    "steueridentifikationsnummer",
    "steuer-id",
    "steuerid",
    "steuerliche identifikationsnummer",
    "steuerliche identifikation",
    "persönliche identifikationsnummer",
    "steuer identifikation",
    "idnr",
    "steuer-idnr",
    "steuernummer",
    "bzst",
];

/// Build the `TAX_ID_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_id_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Steueridentifikationsnummer",
        r"\b[1-9]\d{10}\b",
        Score::from_static(0.5),
    )
    .expect("static DE Steuer-ID pattern compiles");
    Recognizer::new(Entity::TaxIdDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("TaxIdDeuRecognizer")
        .with_validator(Validator::TaxIdDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, tax_id_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(tax_id_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        tax_id_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_tax_id_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("12345678903", &[("12345678903", 1.0)]),
            ("98765432106", &[("98765432106", 1.0)]),
            ("Meine Steuer-ID: 12345678903.", &[("12345678903", 1.0)]),
            ("IdNr. 98765432106 liegt vor.", &[("98765432106", 1.0)]),
            ("12345678901", &[]),
            ("98765432100", &[]),
            ("02345678901", &[]),
            ("1234567890", &[]),
            ("123456789030", &[]),
            ("11111111111", &[]),
            ("11112345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
