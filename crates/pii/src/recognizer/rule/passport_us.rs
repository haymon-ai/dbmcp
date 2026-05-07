//! `PASSPORT_US` recognizer (keyword-context required).

use crate::recognizer::{Category, KeywordValidator, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

const KEYWORDS: &[&str] = &["passport", "travel document"];

/// Build the `PASSPORT_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn passport_us() -> Rule {
    let pattern = Regex::new("US passport", r"(?i)\b[PE]\d{6,8}\b", Score::from_static(0.4))
        .expect("static US passport pattern compiles");
    Rule::new(entity::PASSPORT_US, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PassportUsRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(KEYWORDS)))
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::passport_us;

    fn matches(text: &str) -> Vec<String> {
        let r = passport_us();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_p_prefix() {
        assert_eq!(matches("passport P01234567"), vec!["P01234567"]);
    }

    #[test]
    fn positive_e_prefix() {
        assert_eq!(matches("travel document E1234567"), vec!["E1234567"]);
    }

    #[test]
    fn negative_no_keyword() {
        assert!(matches("ticket P01234567").is_empty());
    }

    #[test]
    fn negative_wrong_letter() {
        assert!(matches("passport Q01234567").is_empty());
    }
}
