//! `ID_CARD_DE` recognizer (Personalausweisnummer, nPA + legacy T-format).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for DE Personalausweis.
const CONTEXT: &[&str] = &[
    "personalausweis",
    "ausweis",
    "personalausweisnummer",
    "ausweisnummer",
    "ausweisdokument",
    "dokumentennummer",
    "seriennummer",
    "npa",
    "neuer personalausweis",
    "personalausweisgesetz",
    "pauwsg",
    "bundespersonalausweis",
    "identity card",
    "national id",
];

/// Build the `ID_CARD_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn id_card_deu() -> Recognizer {
    let patterns = vec![
        Pattern::new(
            "DE Personalausweisnummer (nPA, ICAO charset)",
            r"(?i)\b[CFGHJKLMNPRTVWXYZ][CFGHJKLMNPRTVWXYZ0-9]{7}[0-9]\b",
            Score::from_static(0.4),
        )
        .expect("static DE nPA pattern compiles"),
        Pattern::new(
            "DE Personalausweisnummer (legacy T + 8 digits)",
            r"(?i)\bT\d{8}\b",
            Score::from_static(0.5),
        )
        .expect("static DE legacy ID pattern compiles"),
    ];
    Recognizer::new(Entity::IdCardDe, patterns)
        .expect("non-empty pattern list")
        .with_name("IdCardDeuRecognizer")
        .with_validator(Validator::IdCardDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::id_card_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        let mut spans: Vec<(usize, usize)> = id_card_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        spans
    }

    #[test]
    fn recognizes_id_card_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("L01X00T44", &[(0, 9)]),
            ("C01234565", &[(0, 9)]),
            ("CZ6311T03", &[(0, 9)]),
            ("G00000002", &[(0, 9)]),
            ("Personalausweis: L01X00T44.", &[(17, 26)]),
            ("l01x00t44", &[(0, 9)]),
            ("T22000129", &[(0, 9)]),
            ("T00000000", &[(0, 9)]),
            ("T99999999", &[(0, 9)]),
            ("Ausweis Nr. T22000129 gültig bis 2025.", &[(12, 21)]),
            ("t22000129", &[(0, 9)]),
            ("L01X00T47", &[]),
            ("C01234567", &[]),
            ("T2200012", &[]),
            ("T220001290", &[]),
            ("123456789", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
