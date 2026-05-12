//! `COMMERCIAL_REGISTER_DE` recognizer (Handelsregisternummer, HRA/HRB prefix).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for DE Handelsregisternummer.
const CONTEXT: &[&str] = &[
    "handelsregister",
    "handelsregisternummer",
    "amtsgericht",
    "registergericht",
    "hra",
    "hrb",
    "hr-nummer",
    "registerauszug",
    "handelsregistereintrag",
    "firma",
    "gesellschaft",
    "gmbh",
    "ag",
    "ug",
    "kg",
    "ohg",
    "einzelkaufmann",
    "einzelkauffrau",
    "handelsregisterblattnummer",
];

/// Build the `COMMERCIAL_REGISTER_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn commercial_register_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Handelsregisternummer",
        r"(?i)\bHR[AB]\s*\d{1,6}\b",
        Score::from_static(0.5),
    )
    .expect("static DE commercial register pattern compiles");
    Recognizer::new(Entity::CommercialRegisterDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("CommercialRegisterDeuRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::commercial_register_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        commercial_register_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_commercial_register_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("HRB 123456", &[(0, 10)]),
            ("HRB 1", &[(0, 5)]),
            ("HRB123456", &[(0, 9)]),
            ("HRA 12345", &[(0, 9)]),
            ("HRA12345", &[(0, 8)]),
            ("Amtsgericht München HRB 12345.", &[(21, 30)]),
            ("eingetragen im HRA 99999 Köln", &[(15, 24)]),
            ("Handelsregisternummer: HRB 123456", &[(23, 33)]),
            ("HRB 999999", &[(0, 10)]),
            ("hrb 12345", &[(0, 9)]),
            ("HRC 12345", &[]),
            ("HR 12345", &[]),
            ("HRB 1234567", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
