//! `CREDIT_CARD` recognizer with Luhn checksum validator.

use crate::recognizers::prelude::*;

/// Context keywords boosted by the context-aware scoring pass.
const CONTEXT: &[&str] = &[
    "credit",
    "card",
    "visa",
    "mastercard",
    "cc",
    "amex",
    "discover",
    "jcb",
    "diners",
    "maestro",
    "instapayment",
];

/// Build the `CREDIT_CARD` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn credit_card() -> Recognizer {
    let pattern = Pattern::new(
        "All Credit Cards (weak)",
        r"\b(?!1\d{12}(?!\d))((4\d{3})|(5[0-5]\d{2})|(6\d{3})|(1\d{3})|(3\d{3}))[- ]?(\d{3,4})[- ]?(\d{3,4})[- ]?(\d{3,5})\b",
        Score::from_static(0.3),
    )
    .expect("static credit-card pattern compiles");
    Recognizer::new(Entity::CreditCard, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("CreditCardRecognizer")
        .with_validator(Validator::Luhn)
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, credit_card};

    #[test]
    fn carries_context_list() {
        assert_eq!(credit_card().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        credit_card()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_credit_card() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            (
                "4012888888881881 4012-8888-8888-1881 4012 8888 8888 1881",
                &[
                    ("4012888888881881", 1.0),
                    ("4012-8888-8888-1881", 1.0),
                    ("4012 8888 8888 1881", 1.0),
                ],
            ),
            ("1748503543012", &[]),
            ("122000000000003", &[("122000000000003", 1.0)]),
            ("my credit card: 122000000000003", &[("122000000000003", 1.0)]),
            ("371449635398431", &[("371449635398431", 1.0)]),
            ("5555555555554444", &[("5555555555554444", 1.0)]),
            ("5019717010103742", &[("5019717010103742", 1.0)]),
            ("30569309025904", &[("30569309025904", 1.0)]),
            ("6011000400000000", &[("6011000400000000", 1.0)]),
            ("3528000700000000", &[("3528000700000000", 1.0)]),
            ("6759649826438453", &[("6759649826438453", 1.0)]),
            ("5555555555554444", &[("5555555555554444", 1.0)]),
            ("4111111111111111", &[("4111111111111111", 1.0)]),
            ("4917300800000000", &[("4917300800000000", 1.0)]),
            ("4484070000000000", &[("4484070000000000", 1.0)]),
            ("4012-8888-8888-1882", &[]),
            ("my credit card number is 4012-8888-8888-1882", &[]),
            ("36168002586008", &[]),
            ("my credit card number is 36168002586008", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
