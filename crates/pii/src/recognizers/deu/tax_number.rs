//! `TAX_NUMBER_DE` recognizer (Steuernummer, ELSTER + state-specific slash formats).

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, tax_number_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(tax_number_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        let mut hits: Vec<(usize, usize, f32)> = tax_number_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end, r.score.as_f32()))
            .collect();
        // The slash formats overlap (3/3/5 vs general) on one span at different scores; keep the highest.
        hits.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(b.2.total_cmp(&a.2)));
        hits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        hits.into_iter().map(|(s, e, score)| (&text[s..e], score)).collect()
    }

    #[test]
    fn recognizes_tax_number_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("0281508150123", &[("0281508150123", 0.5)]),
            ("0981508150999", &[("0981508150999", 0.5)]),
            ("1681508150001", &[("1681508150001", 0.5)]),
            ("0181508150000", &[("0181508150000", 0.5)]),
            ("123/456/78901", &[("123/456/78901", 0.4)]),
            ("987/654/32100", &[("987/654/32100", 0.4)]),
            ("12/345/6789", &[("12/345/6789", 0.2)]),
            ("12/3456/7890", &[("12/3456/7890", 0.2)]),
            ("123/3456/7890", &[("123/3456/7890", 0.2)]),
            ("Steuernummer: 0981508150999 wurde vergeben.", &[("0981508150999", 0.5)]),
            ("St.-Nr. 123/456/78901 bitte angeben.", &[("123/456/78901", 0.4)]),
            ("1781508150001", &[]),
            ("0081508150001", &[]),
            ("028150815012", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
