//! `POSTCODE_UK` recognizer (six standard formats plus special GIR 0AA).

use crate::recognizers::prelude::*;

/// Context keywords for UK postcode.
const CONTEXT: &[&str] = &[
    "postcode",
    "post code",
    "postal code",
    "zip",
    "address",
    "delivery",
    "mailing",
    "shipping",
    "correspondence",
];

/// Build the `POSTCODE_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn postcode_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK Postcode",
        r"(?i)\b(GIR\s?0AA|[A-PR-UWYZ][0-9][ABCDEFGHJKPSTUW]?\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][0-9]{2}\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][A-HK-Y][0-9][ABEHMNPRVWXY]?\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][A-HK-Y][0-9]{2}\s?[0-9][ABD-HJLNP-UW-Z]{2})\b",
        Score::from_static(0.1),
    )
    .expect("static UK postcode pattern compiles");
    Recognizer::new(Entity::PostcodeUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PostcodeGbrRecognizer")
        .with_category(Category::Contact)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, postcode_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(postcode_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        postcode_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_postcode_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("M1 1AA", &[("M1 1AA", 0.1)]),
            ("M60 1NW", &[("M60 1NW", 0.1)]),
            ("W1A 1HQ", &[("W1A 1HQ", 0.1)]),
            ("CR2 6XH", &[("CR2 6XH", 0.1)]),
            ("DN55 1PT", &[("DN55 1PT", 0.1)]),
            ("EC1A 1BB", &[("EC1A 1BB", 0.1)]),
            ("GIR 0AA", &[("GIR 0AA", 0.1)]),
            ("M11AA", &[("M11AA", 0.1)]),
            ("EC1A1BB", &[("EC1A1BB", 0.1)]),
            ("DN551PT", &[("DN551PT", 0.1)]),
            ("GIR0AA", &[("GIR0AA", 0.1)]),
            ("My address is SW1A 1AA in London", &[("SW1A 1AA", 0.1)]),
            ("Send to postcode EC2A 1NT please", &[("EC2A 1NT", 0.1)]),
            ("From SW1A 1AA to EC1A 1BB", &[("SW1A 1AA", 0.1), ("EC1A 1BB", 0.1)]),
            ("QA1 1AA", &[]),
            ("VA1 1AA", &[]),
            ("XA1 1AA", &[]),
            ("M1 1CA", &[]),
            ("M1 1AI", &[]),
            ("1A1 1AA", &[]),
            ("ABCM11AADEF", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
