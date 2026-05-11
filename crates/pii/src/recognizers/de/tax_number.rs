//! `TAX_NUMBER_DE` recognizer (Steuernummer, ELSTER + state-specific slash formats).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `TAX_NUMBER_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_number_de() -> Recognizer {
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
        .with_name("TaxNumberDeRecognizer")
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::tax_number_de;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        let mut spans: Vec<(usize, usize)> = tax_number_de()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        spans
    }

    #[test]
    fn recognizes_tax_number_de() {
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
