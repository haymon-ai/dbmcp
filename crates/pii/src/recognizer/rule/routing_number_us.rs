//! `ROUTING_NUMBER_US` recognizer (ABA checksum + keyword-context).

use crate::recognizer::{Category, KeywordValidator, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

const KEYWORDS: &[&str] = &["routing", "aba", "rtn", "bank"];

/// Build the `ROUTING_NUMBER_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn routing_number_us() -> Rule {
    let pattern = Regex::new("US ABA routing", r"\b\d{9}\b", Score::from_static(0.4))
        .expect("static ABA routing pattern compiles");
    let validator = Validator::And(
        Box::new(Validator::AbaRouting),
        Box::new(Validator::Keyword(KeywordValidator::new(KEYWORDS))),
    );
    Rule::new(entity::ROUTING_NUMBER_US, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("RoutingNumberUsRecognizer")
        .with_validator(validator)
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::routing_number_us;

    fn matches(text: &str) -> Vec<String> {
        let r = routing_number_us();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_valid_aba() {
        // 021000021 — JPMorgan Chase ABA routing (valid checksum).
        assert_eq!(matches("bank routing 021000021"), vec!["021000021"]);
    }

    #[test]
    fn negative_no_keyword() {
        assert!(matches("version 021000021").is_empty());
    }

    #[test]
    fn negative_checksum_perturbations() {
        let bad = [
            "021000020",
            "021000022",
            "021000023",
            "021000024",
            "021000025",
            "021000026",
            "021000027",
            "021000028",
            "021000029",
            "021100021",
            "022000021",
            "121000021",
        ];
        for n in bad {
            let text = format!("bank routing {n}");
            assert!(
                matches(&text).is_empty(),
                "{n} has invalid ABA checksum, expected no match"
            );
        }
    }
}
