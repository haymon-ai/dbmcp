//! `ID_CARD_DE` recognizer (Personalausweisnummer, nPA + legacy T-format).

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, id_card_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(id_card_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        let mut hits: Vec<(usize, usize, f32)> = id_card_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end, r.score.as_f32()))
            .collect();
        // The nPA and legacy-T patterns can match one span at different scores; keep the highest.
        hits.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(b.2.total_cmp(&a.2)));
        hits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        hits.into_iter().map(|(s, e, score)| (&text[s..e], score)).collect()
    }

    #[test]
    fn recognizes_id_card_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("L01X00T44", &[("L01X00T44", 1.0)]),
            ("C01234565", &[("C01234565", 1.0)]),
            ("CZ6311T03", &[("CZ6311T03", 1.0)]),
            ("G00000002", &[("G00000002", 1.0)]),
            ("Personalausweis: L01X00T44.", &[("L01X00T44", 1.0)]),
            ("l01x00t44", &[("l01x00t44", 1.0)]),
            ("T22000129", &[("T22000129", 0.5)]),
            ("T00000000", &[("T00000000", 0.5)]),
            ("T99999999", &[("T99999999", 0.5)]),
            ("Ausweis Nr. T22000129 gültig bis 2025.", &[("T22000129", 0.5)]),
            ("t22000129", &[("t22000129", 0.5)]),
            ("L01X00T47", &[]),
            ("C01234567", &[]),
            ("T2200012", &[]),
            ("T220001290", &[]),
            ("123456789", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
