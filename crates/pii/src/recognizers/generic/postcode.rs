//! `POSTCODE` recognizer (country-neutral 5-digit postcode, keyword context required).
//!
//! A bare 5-digit code is valid in dozens of countries (Germany, France, Spain,
//! Italy, the US, …), so a match is never tied to one country. Ties with
//! `POSTCODE_DE` are settled in `recognizers::all()`.

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
    // Every 5-digit code but `00000`, spelled without lookaround so fancy-regex
    // hands it to the faster `regex` engine.
    let pattern = Pattern::new(
        "Postcode",
        r"\b(?:[1-9]\d{4}|0[1-9]\d{3}|00[1-9]\d{2}|000[1-9]\d|0000[1-9])\b",
        Score::from_static(0.05),
    )
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
