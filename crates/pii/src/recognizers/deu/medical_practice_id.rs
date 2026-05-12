//! `MEDICAL_PRACTICE_ID_DE` recognizer (Betriebsstättennummer / BSNR — 9 digits, all-zero rejected).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for DE Betriebsstättennummer (BSNR).
const CONTEXT: &[&str] = &[
    "betriebsstättennummer",
    "betriebsstätten-nummer",
    "bsnr",
    "betriebsstätte",
    "praxisnummer",
    "arztpraxis",
    "praxis",
    "kassenärztliche vereinigung",
    "kv-nummer",
    "kv nummer",
    "praxisadresse",
    "praxisstandort",
    "nebenbetriebsstätte",
    "hauptbetriebsstätte",
    "behandlungsort",
    "vertragsarztpraxis",
];

/// Build the `MEDICAL_PRACTICE_ID_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn medical_practice_id_deu() -> Recognizer {
    let pattern = Pattern::new("DE Medical Practice ID", r"\b\d{9}\b", Score::from_static(0.2))
        .expect("static DE medical practice ID pattern compiles");
    Recognizer::new(Entity::MedicalPracticeIdDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("MedicalPracticeIdDeuRecognizer")
        .with_validator(Validator::MedicalPracticeIdDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::medical_practice_id_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        medical_practice_id_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_medical_practice_id_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("021234568", &[(0, 9)]),
            ("521234567", &[(0, 9)]),
            ("711234567", &[(0, 9)]),
            ("351234567", &[(0, 9)]),
            ("991234567", &[(0, 9)]),
            ("051234567", &[(0, 9)]),
            ("Betriebsstättennummer: 021234568", &[(24, 33)]),
            ("BSNR 711234567 der Praxis.", &[(5, 14)]),
            ("000000000", &[]),
            ("02123456", &[]),
            ("0212345689", &[]),
            ("02123456A", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
