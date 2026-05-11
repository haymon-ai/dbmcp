//! `SOCIAL_SECURITY_DE` recognizer (Rentenversicherungsnummer / RVNR).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `SOCIAL_SECURITY_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn social_security_de() -> Recognizer {
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
        .with_name("SocialSecurityDeRecognizer")
        .with_validator(Validator::DeSocialSecurity)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::social_security_de;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        let mut spans: Vec<(usize, usize)> = social_security_de()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        spans
    }

    #[test]
    fn recognizes_social_security_de() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("15070649C103", &[(0, 12)]),
            ("65070803A019", &[(0, 12)]),
            ("20151090B023", &[(0, 12)]),
            ("38551285K051", &[(0, 12)]),
            ("RVNR: 15070649C103 laut Sozialversicherungsausweis.", &[(6, 18)]),
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
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
