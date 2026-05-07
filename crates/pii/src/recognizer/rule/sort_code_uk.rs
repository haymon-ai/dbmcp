//! `SORT_CODE_UK` recognizer (keyword-context required).

use crate::recognizer::{Category, KeywordValidator, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

const KEYWORDS: &[&str] = &["sort", "sortcode", "sort code"];

/// Build the `SORT_CODE_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn sort_code_uk() -> Rule {
    let pattern = Regex::new(
        "UK sort code",
        r"\b\d{2}[- ]?\d{2}[- ]?\d{2}\b",
        Score::from_static(0.4),
    )
    .expect("static UK sort-code pattern compiles");
    Rule::new(entity::SORT_CODE_UK, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SortCodeUkRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(KEYWORDS)))
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::sort_code_uk;

    fn matches(text: &str) -> Vec<String> {
        let r = sort_code_uk();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_dashed_with_keyword() {
        assert_eq!(matches("sort 12-34-56"), vec!["12-34-56"]);
    }

    #[test]
    fn positive_spaced_with_keyword() {
        assert_eq!(matches("sort code 12 34 56"), vec!["12 34 56"]);
    }

    #[test]
    fn negative_no_keyword() {
        assert!(matches("transaction 12-34-56").is_empty());
    }
}
