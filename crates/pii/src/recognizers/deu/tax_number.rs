//! `TAX_NUMBER_DE` recognizer (Steuernummer, ELSTER + state-specific slash formats).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for DE Steuernummer.
const CONTEXT: &[&str] = &[
    "steuernummer",
    "steuer-nr",
    "steuer nr",
    "st.-nr",
    "st-nr",
    "finanzamt",
    "umsatzsteuer",
    "einkommensteuer",
    "körperschaftsteuer",
    "gewerbesteuer",
    "steuerveranlagung",
    "steuerbescheid",
];

/// Build the `TAX_NUMBER_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_number_deu() -> Recognizer {
    let patterns = vec![
        Pattern::new(
            "DE Steuernummer (ELSTER 13-digit)",
            r"\b(0[1-9]|1[0-6])\d{11}\b",
            Score::from_static(0.5),
        )
        .expect("static DE Steuernummer ELSTER pattern compiles"),
        Pattern::new(
            "DE Steuernummer (Bayern/BW 3/3/5)",
            r"(?<!\w)\d{3}/\d{3}/\d{5}(?!\w)",
            Score::from_static(0.4),
        )
        .expect("static DE Steuernummer 3/3/5 pattern compiles"),
        Pattern::new(
            "DE Steuernummer (general 2-3/3-4/4-5)",
            r"(?<!\w)\d{2,3}/\d{3,4}/\d{4,5}(?!\w)",
            Score::from_static(0.2),
        )
        .expect("static DE Steuernummer general pattern compiles"),
    ];
    Recognizer::new(Entity::TaxNumberDe, patterns)
        .expect("non-empty pattern list")
        .with_name("TaxNumberDeuRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::tax_number_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        let mut spans: Vec<(usize, usize)> = tax_number_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        spans
    }

    #[test]
    fn recognizes_tax_number_deu() {
        // Loose 2-3/3-4/4-5 pattern overlaps the strict 3/3/5 — spans dedup'd in helper.
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("0281508150123", &[(0, 13)]),
            ("0981508150999", &[(0, 13)]),
            ("1681508150001", &[(0, 13)]),
            ("0181508150000", &[(0, 13)]),
            ("123/456/78901", &[(0, 13)]),
            ("987/654/32100", &[(0, 13)]),
            ("12/345/6789", &[(0, 11)]),
            ("12/3456/7890", &[(0, 12)]),
            ("123/3456/7890", &[(0, 13)]),
            ("Steuernummer: 0981508150999 wurde vergeben.", &[(14, 27)]),
            ("St.-Nr. 123/456/78901 bitte angeben.", &[(8, 21)]),
            ("1781508150001", &[]),
            ("0081508150001", &[]),
            ("028150815012", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
