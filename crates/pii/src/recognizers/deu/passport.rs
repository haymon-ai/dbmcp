//! `PASSPORT_DE` recognizer (Reisepassnummer, ICAO Doc 9303 9-character format).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for DE Reisepass.
const CONTEXT: &[&str] = &[
    "reisepass",
    "pass",
    "passnummer",
    "reisepassnummer",
    "passport",
    "passport number",
    "pass-nr",
    "dokumentennummer",
    "bundesrepublik deutschland",
    "ausweisdokument",
    "mrz",
];

/// Build the `PASSPORT_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn passport_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Reisepassnummer (ICAO charset)",
        r"(?i)\b[CFGHJKLMNPRTVWXYZ][CFGHJKLMNPRTVWXYZ0-9]{7}[0-9]\b",
        Score::from_static(0.4),
    )
    .expect("static DE passport pattern compiles");
    Recognizer::new(Entity::PassportDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PassportDeuRecognizer")
        .with_validator(Validator::IcaoMrz9)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, passport_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(passport_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        passport_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_passport_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("C01234565", &[("C01234565", 1.0)]),
            ("F12345671", &[("F12345671", 1.0)]),
            ("L01X00T44", &[("L01X00T44", 1.0)]),
            ("CZ6311T03", &[("CZ6311T03", 1.0)]),
            ("G00000002", &[("G00000002", 1.0)]),
            ("C01X00T41", &[("C01X00T41", 1.0)]),
            ("Reisepass C01234565 ausgestellt am 01.01.2020.", &[("C01234565", 1.0)]),
            ("Pass-Nr.: F12345671", &[("F12345671", 1.0)]),
            ("C01234567", &[]),
            ("F12345678", &[]),
            ("L01X00T47", &[]),
            ("c01234565", &[("c01234565", 1.0)]),
            ("C0123456", &[]),
            ("C012345678", &[]),
            ("901234567", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
