//! `POSTCODE` recognizer (country-neutral 5-digit postcode, keyword context required).
//!
//! A bare 5-digit code is valid in dozens of countries (Germany, France, Spain,
//! Italy, the US, …), so a match is kept only next to a generic postcode or
//! address keyword and is never tied to one country. Any 5-digit code but
//! `00000` matches, US ZIPs such as `00501` included. German keywords such as
//! `plz` label the same value `POSTCODE_DE` instead; when keywords of both
//! kinds support one value, `POSTCODE_DE` wins the tie by registration order.

use crate::recognizers::prelude::*;

/// Context keywords for a postcode of any country.
const CONTEXT: &[&str] = &[
    "zip",
    "postcode",
    "post code",
    "postal",
    "address",
    "adresse",
    "city",
    "delivery",
    "mailing",
    "shipping",
    "correspondence",
];

/// Build the `POSTCODE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn postcode() -> Recognizer {
    let pattern = Pattern::new("Postcode", r"\b(?!00000\b)\d{5}\b", Score::from_static(0.05))
        .expect("static postcode pattern compiles")
        .requiring_context();
    Recognizer::new(Entity::Postcode, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PostcodeRecognizer")
        .with_category(Category::Contact)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, postcode};

    #[test]
    fn carries_context_list() {
        assert_eq!(postcode().context(), CONTEXT);
    }

    #[test]
    fn requires_context() {
        assert!(
            postcode()
                .patterns()
                .iter()
                .all(crate::pattern::Pattern::requires_context)
        );
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        postcode()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_postcode() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("75008", &[("75008", 0.05)]),
            ("10115", &[("10115", 0.05)]),
            ("00501", &[("00501", 0.05)]),
            ("01000", &[("01000", 0.05)]),
            ("99999", &[("99999", 0.05)]),
            ("12 rue X, 75008 Paris", &[("75008", 0.05)]),
            ("00000", &[]),
            ("750080", &[]),
            ("7500", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
