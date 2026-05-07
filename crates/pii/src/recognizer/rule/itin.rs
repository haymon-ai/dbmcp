//! `ITIN` recognizer (US Individual Taxpayer Identification Number).

use crate::recognizer::{Category, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `ITIN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn itin() -> Rule {
    let pattern = Regex::new("US ITIN", r"\b9\d{2}-?[789]\d-?\d{4}\b", Score::from_static(0.5))
        .expect("static ITIN pattern compiles");
    Rule::new(entity::ITIN, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("ItinRecognizer")
        .with_validator(Validator::ItinRange)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::itin;

    fn matches(text: &str) -> Vec<String> {
        let r = itin();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_dashed() {
        assert_eq!(matches("ITIN 912-72-1234"), vec!["912-72-1234"]);
    }

    #[test]
    fn positive_run() {
        assert_eq!(matches("tax id 912921234"), vec!["912921234"]);
    }

    #[test]
    fn negative_middle_out_of_range() {
        let bad = [
            "912-50-1234",
            "912-69-1234",
            "912-89-1234",
            "912-93-1234",
            "912-00-1234",
            "912-10-1234",
            "912-20-1234",
            "912-30-1234",
            "912-40-1234",
            "912-99x1234",
            "912-60-1234",
        ];
        for n in bad {
            assert!(
                matches(n).is_empty(),
                "{n} has out-of-range middle block, expected no match"
            );
        }
    }

    #[test]
    fn negative_wrong_first_digit() {
        assert!(matches("812-72-1234").is_empty());
    }
}
