//! `PASSPORT_UK` recognizer (keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const KEYWORDS: &[&str] = &["passport", "travel document"];

/// Build the `PASSPORT_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn passport_uk() -> Recognizer {
    let pattern = Pattern::new("UK passport (9 digits)", r"\b\d{9}\b", Score::from_static(0.4))
        .expect("static UK passport pattern compiles");
    Recognizer::new(Entity::PassportUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PassportUkRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(KEYWORDS)))
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::passport_uk;

    fn matches(text: &str) -> Vec<String> {
        let r = passport_uk();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_with_keyword() {
        assert_eq!(matches("Passport: 925076473"), vec!["925076473"]);
    }

    #[test]
    fn negative_no_keyword() {
        assert!(matches("ticket 925076473").is_empty());
    }
}
