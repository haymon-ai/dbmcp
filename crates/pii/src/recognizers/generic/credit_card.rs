//! `CREDIT_CARD` recognizer with Luhn checksum validator.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

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
    use super::credit_card;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        credit_card()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_credit_cards() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            (
                "4012888888881881 4012-8888-8888-1881 4012 8888 8888 1881",
                &[(0, 16), (17, 36), (37, 56)],
            ),
            ("1748503543012", &[]),
            ("122000000000003", &[(0, 15)]),
            ("my credit card: 122000000000003", &[(16, 31)]),
            ("371449635398431", &[(0, 15)]),
            ("5555555555554444", &[(0, 16)]),
            ("5019717010103742", &[(0, 16)]),
            ("30569309025904", &[(0, 14)]),
            ("6011000400000000", &[(0, 16)]),
            ("3528000700000000", &[(0, 16)]),
            ("6759649826438453", &[(0, 16)]),
            ("5555555555554444", &[(0, 16)]),
            ("4111111111111111", &[(0, 16)]),
            ("4917300800000000", &[(0, 16)]),
            ("4484070000000000", &[(0, 16)]),
            ("4012-8888-8888-1882", &[]),
            ("my credit card number is 4012-8888-8888-1882", &[]),
            ("36168002586008", &[]),
            ("my credit card number is 36168002586008", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
