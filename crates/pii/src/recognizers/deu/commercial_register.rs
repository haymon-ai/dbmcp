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
    use super::{CONTEXT, commercial_register_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(commercial_register_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        commercial_register_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_commercial_register_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("HRB 123456", &[("HRB 123456", 0.5)]),
            ("HRB 1", &[("HRB 1", 0.5)]),
            ("HRB123456", &[("HRB123456", 0.5)]),
            ("HRA 12345", &[("HRA 12345", 0.5)]),
            ("HRA12345", &[("HRA12345", 0.5)]),
            ("Amtsgericht München HRB 12345.", &[("HRB 12345", 0.5)]),
            ("eingetragen im HRA 99999 Köln", &[("HRA 99999", 0.5)]),
            ("Handelsregisternummer: HRB 123456", &[("HRB 123456", 0.5)]),
            ("HRB 999999", &[("HRB 999999", 0.5)]),
            ("hrb 12345", &[("hrb 12345", 0.5)]),
            ("HRC 12345", &[]),
            ("HR 12345", &[]),
            ("HRB 1234567", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
