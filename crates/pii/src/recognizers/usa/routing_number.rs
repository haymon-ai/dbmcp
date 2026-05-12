//! `ROUTING_NUMBER_US` recognizer (ABA checksum + keyword-context).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for US ABA routing number.
const CONTEXT: &[&str] = &["aba", "routing", "abarouting", "association", "bankrouting"];

/// Build the `ROUTING_NUMBER_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn routing_number_usa() -> Recognizer {
    let pattern = Pattern::new("US ABA routing", r"\b\d{9}\b", Score::from_static(0.4))
        .expect("static ABA routing pattern compiles");
    Recognizer::new(Entity::RoutingNumberUs, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("RoutingNumberUsaRecognizer")
        .with_validator(Validator::AbaRoutingUsa)
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::routing_number_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        routing_number_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_routing_number_usa() {
        // 021000021 — JPMorgan Chase ABA routing (valid checksum).
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("bank routing 021000021", &[(13, 22)]),
            ("aba 021000021", &[(4, 13)]),
            ("rtn=021000021", &[(4, 13)]),
            ("version 021000021", &[(8, 17)]),
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
