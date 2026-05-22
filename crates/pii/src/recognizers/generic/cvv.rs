//! `CVV` recognizer (keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `CVV` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn cvv() -> Recognizer {
    let pattern =
        Pattern::new("CVV (3-4 digits)", r"\b\d{3,4}\b", Score::from_static(0.3)).expect("static CVV pattern compiles");
    Recognizer::new(Entity::Cvv, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("CvvRecognizer")
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::cvv;

    fn results(text: &str) -> Vec<(&str, f32)> {
        cvv()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_cvv() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("cvv: 123", &[("123", 0.3)]),
            ("cvc 4567", &[("4567", 0.3)]),
            ("CSC=789", &[("789", 0.3)]),
            ("cvv 12", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
