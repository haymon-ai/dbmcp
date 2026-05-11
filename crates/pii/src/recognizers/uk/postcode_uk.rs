//! `POSTCODE_UK` recognizer (six standard formats plus special GIR 0AA).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `POSTCODE_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn postcode_uk() -> Recognizer {
    let pattern = Pattern::new(
        "UK Postcode",
        r"(?i)\b(GIR\s?0AA|[A-PR-UWYZ][0-9][ABCDEFGHJKPSTUW]?\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][0-9]{2}\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][A-HK-Y][0-9][ABEHMNPRVWXY]?\s?[0-9][ABD-HJLNP-UW-Z]{2}|[A-PR-UWYZ][A-HK-Y][0-9]{2}\s?[0-9][ABD-HJLNP-UW-Z]{2})\b",
        Score::from_static(0.1),
    )
    .expect("static UK postcode pattern compiles");
    Recognizer::new(Entity::PostcodeUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PostcodeUkRecognizer")
        .with_category(Category::Contact)
}

#[cfg(test)]
mod tests {
    use super::postcode_uk;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        postcode_uk()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_postcode_uk() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("M1 1AA", &[(0, 6)]),
            ("M60 1NW", &[(0, 7)]),
            ("W1A 1HQ", &[(0, 7)]),
            ("CR2 6XH", &[(0, 7)]),
            ("DN55 1PT", &[(0, 8)]),
            ("EC1A 1BB", &[(0, 8)]),
            ("GIR 0AA", &[(0, 7)]),
            ("M11AA", &[(0, 5)]),
            ("EC1A1BB", &[(0, 7)]),
            ("DN551PT", &[(0, 7)]),
            ("GIR0AA", &[(0, 6)]),
            ("My address is SW1A 1AA in London", &[(14, 22)]),
            ("Send to postcode EC2A 1NT please", &[(17, 25)]),
            ("From SW1A 1AA to EC1A 1BB", &[(5, 13), (17, 25)]),
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
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
