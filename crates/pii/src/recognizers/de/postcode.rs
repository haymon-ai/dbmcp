//! `POSTCODE_DE` recognizer (Postleitzahl / PLZ, weak 0.05 base score — requires context).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `POSTCODE_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn postcode_de() -> Recognizer {
    let pattern = Pattern::new(
        "DE Postcode",
        r"\b(?!01000\b|99999\b)(0[1-9]\d{3}|[1-9]\d{4})\b",
        Score::from_static(0.05),
    )
    .expect("static DE postcode pattern compiles");
    Recognizer::new(Entity::PostcodeDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PostcodeDeRecognizer")
        .with_category(Category::Contact)
}

#[cfg(test)]
mod tests {
    use super::postcode_de;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        postcode_de()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_postcode_de() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("10115", &[(0, 5)]),
            ("80331", &[(0, 5)]),
            ("22085", &[(0, 5)]),
            ("01001", &[(0, 5)]),
            ("99998", &[(0, 5)]),
            ("PLZ: 10115", &[(5, 10)]),
            ("Postleitzahl 80331 München", &[(13, 18)]),
            ("00000", &[]),
            ("01000", &[]),
            ("99999", &[]),
            ("101150", &[]),
            ("1011", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
