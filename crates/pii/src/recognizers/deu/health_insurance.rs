//! `HEALTH_INSURANCE_DE` recognizer (KVNR, letter + 9 digits, GKV-Spitzenverband checksum).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

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
    use super::health_insurance_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        health_insurance_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_health_insurance_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("A000500015", &[(0, 10)]),
            ("C000500021", &[(0, 10)]),
            ("A123456780", &[(0, 10)]),
            ("M123456785", &[(0, 10)]),
            ("B123456782", &[(0, 10)]),
            ("Z000000005", &[(0, 10)]),
            ("Z999999997", &[(0, 10)]),
            ("Krankenkasse KVNR: A123456780", &[(19, 29)]),
            ("eGK-Nummer M123456785 bitte angeben.", &[(11, 21)]),
            ("a123456780", &[(0, 10)]),
            ("A123456787", &[]),
            ("M123456789", &[]),
            ("1123456780", &[]),
            ("A12345678", &[]),
            ("A1234567890", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
