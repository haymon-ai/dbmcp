//! `ROUTING_NUMBER_US` recognizer (ABA checksum + keyword-context).

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, routing_number_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(routing_number_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        routing_number_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_routing_number_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("bank routing 021000021", &[("021000021", 1.0)]),
            ("aba 021000021", &[("021000021", 1.0)]),
            ("rtn=021000021", &[("021000021", 1.0)]),
            ("version 021000021", &[("021000021", 1.0)]),
            ("bank routing 021000020", &[]),
            ("bank routing 121000021", &[]),
            ("bank routing 12345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
