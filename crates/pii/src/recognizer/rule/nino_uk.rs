//! `NINO_UK` recognizer (UK National Insurance Number with prefix blocklist).

use crate::recognizer::{Category, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `NINO_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn nino_uk() -> Rule {
    let pattern = Regex::new(
        "UK NINO",
        r"(?i)\b[A-Z]{2}[ -]?\d{2}[ -]?\d{2}[ -]?\d{2}[ -]?[A-D]?\b",
        Score::from_static(0.4),
    )
    .expect("static NINO pattern compiles");
    Rule::new(entity::NINO_UK, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("NinoUkRecognizer")
        .with_validator(Validator::NinoBlocklist)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::nino_uk;

    fn matches(text: &str) -> Vec<String> {
        let r = nino_uk();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_valid_with_suffix() {
        assert_eq!(matches("NI number AB123456C"), vec!["AB123456C"]);
    }

    #[test]
    fn positive_no_suffix() {
        assert_eq!(matches("NI AB123456"), vec!["AB123456"]);
    }

    #[test]
    fn negative_blocked_prefix() {
        assert!(matches("NI BG123456C").is_empty());
        assert!(matches("NI ZZ123456C").is_empty());
    }

    #[test]
    fn negative_o_in_second_position() {
        assert!(matches("NI AO123456C").is_empty());
    }

    #[test]
    fn negative_invalid_suffix_letter() {
        assert!(matches("NI AB123456E").is_empty());
    }
}
