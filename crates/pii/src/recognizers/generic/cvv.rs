//! `CVV` recognizer (keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const KEYWORDS: &[&str] = &["cvv", "cvc", "csc", "security code"];

/// Build the `CVV` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn cvv() -> Recognizer {
    let pattern =
        Pattern::new("CVV (3-4 digits)", r"\b\d{3,4}\b", Score::from_static(0.3)).expect("static CVV pattern compiles");
    Recognizer::new(Entity::Cvv, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("CvvRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(KEYWORDS)))
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::cvv;

    fn matches(text: &str) -> Vec<String> {
        let r = cvv();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_three_digit() {
        assert_eq!(matches("cvv: 123"), vec!["123"]);
    }

    #[test]
    fn positive_four_digit() {
        assert_eq!(matches("cvc 4567"), vec!["4567"]);
    }

    #[test]
    fn positive_csc_keyword() {
        assert_eq!(matches("CSC=789"), vec!["789"]);
    }

    #[test]
    fn negative_no_keyword() {
        assert!(matches("port 4567").is_empty());
        assert!(matches("Expires 123").is_empty());
    }

    #[test]
    fn negative_too_short() {
        assert!(matches("cvv 12").is_empty());
    }
}
