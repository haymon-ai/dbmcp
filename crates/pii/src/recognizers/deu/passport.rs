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
    use super::passport_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        passport_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_passport_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("C01234565", &[(0, 9)]),
            ("F12345671", &[(0, 9)]),
            ("L01X00T44", &[(0, 9)]),
            ("CZ6311T03", &[(0, 9)]),
            ("G00000002", &[(0, 9)]),
            ("C01X00T41", &[(0, 9)]),
            ("Reisepass C01234565 ausgestellt am 01.01.2020.", &[(10, 19)]),
            ("Pass-Nr.: F12345671", &[(10, 19)]),
            ("C01234567", &[]),
            ("F12345678", &[]),
            ("L01X00T47", &[]),
            ("c01234565", &[(0, 9)]),
            ("C0123456", &[]),
            ("C012345678", &[]),
            ("901234567", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
