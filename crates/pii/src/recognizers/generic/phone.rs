//! `PHONE_NUMBER` recognizer.
//!
//! Matches an E.164 number: a `+` or IDD `00` prefix, a non-zero country code,
//! then 8–15 significant digits with optional punctuation. The required
//! international prefix keeps bare digit runs (timestamps, IDs, references) out.

use crate::recognizers::prelude::*;

/// Context keywords used by the boost step.
const CONTEXT: &[&str] = &["phone", "number", "telephone", "cell", "cellphone", "mobile", "call"];

/// Build the `PHONE_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn phone_number() -> Recognizer {
    let pattern = Pattern::new(
        "intl-e164",
        r"(?<![\w+])(?:00|\+)[\s.\-/()]*+[1-9](?:[\s.\-/()]*+\d){7,14}",
        Score::from_static(0.4),
    )
    .expect("intl-e164 pattern compiles");
    Recognizer::new(Entity::PhoneNumber, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PhoneRecognizer")
        .with_category(Category::Contact)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, phone_number};

    #[test]
    fn carries_context_list() {
        assert_eq!(phone_number().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        phone_number()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_phone() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("+14155552671", &[("+14155552671", 0.4)]),
            ("+44 20 7946 0958", &[("+44 20 7946 0958", 0.4)]),
            ("+49 30 12345678", &[("+49 30 12345678", 0.4)]),
            ("+43 12345678", &[("+43 12345678", 0.4)]),
            ("+49 36878 620-23924", &[("+49 36878 620-23924", 0.4)]),
            ("+43 5574 6706 0000", &[("+43 5574 6706 0000", 0.4)]),
            ("0049/5235/3-00", &[("0049/5235/3-00", 0.4)]),
            ("0033/1/34317000", &[("0033/1/34317000", 0.4)]),
            ("(415) 555-2671", &[]),
            ("02012345678", &[]),
            ("2021-10-04 09:07:51", &[]),
            ("900000000", &[]),
            ("4111111111111111", &[]),
            ("0461234567", &[]),
            ("00012345", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
