//! `ROUTING_NUMBER_US` recognizer (ABA checksum + keyword-context).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const KEYWORDS: &[&str] = &["routing", "aba", "rtn", "bank"];

/// Build the `ROUTING_NUMBER_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn routing_number_us() -> Recognizer {
    let pattern = Pattern::new("US ABA routing", r"\b\d{9}\b", Score::from_static(0.4))
        .expect("static ABA routing pattern compiles");
    let validator = Validator::And(
        Box::new(Validator::AbaRouting),
        Box::new(Validator::Keyword(KeywordValidator::new(KEYWORDS))),
    );
    Recognizer::new(Entity::RoutingNumberUs, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("RoutingNumberUsRecognizer")
        .with_validator(validator)
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::routing_number_us;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        routing_number_us()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_routing_number_us() {
        // 021000021 — JPMorgan Chase ABA routing (valid checksum).
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("bank routing 021000021", &[(13, 22)]),
            ("aba 021000021", &[(4, 13)]),
            ("rtn=021000021", &[(4, 13)]),
            ("version 021000021", &[]),
            ("bank routing 021000020", &[]),
            ("bank routing 121000021", &[]),
            ("bank routing 12345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
