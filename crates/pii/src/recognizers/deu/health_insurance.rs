//! `HEALTH_INSURANCE_DE` recognizer (KVNR, letter + 9 digits, GKV-Spitzenverband checksum).

use crate::recognizers::prelude::*;

/// Context keywords for DE Krankenversicherungsnummer.
const CONTEXT: &[&str] = &[
    "krankenversicherungsnummer",
    "krankenversichertennummer",
    "versichertennummer",
    "kvnr",
    "krankenkasse",
    "krankenversicherung",
    "gesundheitskarte",
    "egk",
    "elektronische gesundheitskarte",
    "gkv",
    "gesetzliche krankenversicherung",
    "krankenversicherungsausweis",
    "versichertenausweis",
    "versichertenkarte",
    "aok",
    "tkk",
    "barmer",
    "dak",
];

/// Build the `HEALTH_INSURANCE_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn health_insurance_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Krankenversicherungsnummer",
        r"(?i)\b[A-Z]\d{9}\b",
        Score::from_static(0.3),
    )
    .expect("static DE KVNR pattern compiles");
    Recognizer::new(Entity::HealthInsuranceDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("HealthInsuranceDeuRecognizer")
        .with_validator(Validator::HealthInsuranceDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, health_insurance_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(health_insurance_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        health_insurance_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_health_insurance_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("A000500015", &[("A000500015", 1.0)]),
            ("C000500021", &[("C000500021", 1.0)]),
            ("A123456780", &[("A123456780", 1.0)]),
            ("M123456785", &[("M123456785", 1.0)]),
            ("B123456782", &[("B123456782", 1.0)]),
            ("Z000000005", &[("Z000000005", 1.0)]),
            ("Z999999997", &[("Z999999997", 1.0)]),
            ("Krankenkasse KVNR: A123456780", &[("A123456780", 1.0)]),
            ("eGK-Nummer M123456785 bitte angeben.", &[("M123456785", 1.0)]),
            ("a123456780", &[("a123456780", 1.0)]),
            ("A123456787", &[]),
            ("M123456789", &[]),
            ("1123456780", &[]),
            ("A12345678", &[]),
            ("A1234567890", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
