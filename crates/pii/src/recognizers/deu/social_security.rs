//! `SOCIAL_SECURITY_DE` recognizer (Rentenversicherungsnummer / RVNR).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for DE Rentenversicherungsnummer.
const CONTEXT: &[&str] = &[
    "rentenversicherungsnummer",
    "sozialversicherungsnummer",
    "versicherungsnummer",
    "rvnr",
    "svnr",
    "sv-nummer",
    "rente",
    "rentenversicherung",
    "deutsche rentenversicherung",
    "drv",
    "sozialversicherung",
    "sozialversicherungsausweis",
    "rentenausweis",
];

/// Build the `SOCIAL_SECURITY_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn social_security_deu() -> Recognizer {
    let patterns = vec![
        Pattern::new(
            "DE Rentenversicherungsnummer (strict, with birth-date structure)",
            r"(?i)\b\d{2}(0[1-9]|[12]\d|3[01]|5[1-9]|[67]\d|8[01])(0[1-9]|1[0-2])\d{2}[A-Z]\d{2}[0-9]\b",
            Score::from_static(0.5),
        )
        .expect("static DE RVNR strict pattern compiles"),
        Pattern::new(
            "DE Rentenversicherungsnummer (relaxed)",
            r"(?i)\b\d{8}[A-Z]\d{3}\b",
            Score::from_static(0.3),
        )
        .expect("static DE RVNR relaxed pattern compiles"),
    ];
    Recognizer::new(Entity::SocialSecurityDe, patterns)
        .expect("non-empty pattern list")
        .with_name("SocialSecurityDeuRecognizer")
        .with_validator(Validator::SocialSecurityDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, social_security_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(social_security_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        let mut hits: Vec<(usize, usize, f32)> = social_security_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end, r.score.as_f32()))
            .collect();
        // The strict and relaxed patterns can match one span at different scores; keep the highest.
        hits.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(b.2.total_cmp(&a.2)));
        hits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        hits.into_iter().map(|(s, e, score)| (&text[s..e], score)).collect()
    }

    #[test]
    fn recognizes_social_security_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("15070649C103", &[("15070649C103", 1.0)]),
            ("65070803A019", &[("65070803A019", 1.0)]),
            ("20151090B023", &[("20151090B023", 1.0)]),
            ("38551285K051", &[("38551285K051", 1.0)]),
            (
                "RVNR: 15070649C103 laut Sozialversicherungsausweis.",
                &[("15070649C103", 1.0)],
            ),
            ("15070649C100", &[]),
            ("65070803A012", &[]),
            ("15070049C103", &[]),
            ("15071349C103", &[]),
            ("150706491103", &[]),
            ("15070649C10", &[]),
            ("15070649C1030", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
