//! `LIFETIME_PHYSICIAN_NUMBER_DE` recognizer (Lebenslange Arztnummer / LANR, KBV weighted checksum).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `LIFETIME_PHYSICIAN_NUMBER_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn lifetime_physician_number_de() -> Recognizer {
    let pattern = Pattern::new("DE Lifetime Physician Number", r"\b\d{9}\b", Score::from_static(0.3))
        .expect("static DE lifetime physician number pattern compiles");
    Recognizer::new(Entity::LifetimePhysicianNumberDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("LifetimePhysicianNumberDeRecognizer")
        .with_validator(Validator::LifetimePhysicianNumberDe)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::lifetime_physician_number_de;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        lifetime_physician_number_de()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_lifetime_physician_number_de() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("123456601", &[(0, 9)]),
            ("234567701", &[(0, 9)]),
            ("100000601", &[(0, 9)]),
            ("987654401", &[(0, 9)]),
            ("555555501", &[(0, 9)]),
            ("999999901", &[(0, 9)]),
            ("LANR: 123456601 des behandelnden Arztes.", &[(6, 15)]),
            ("Arztnummer 987654401 auf dem Rezept.", &[(11, 20)]),
            ("123456901", &[]),
            ("234567601", &[]),
            ("100000401", &[]),
            ("12345660", &[]),
            ("1234566010", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
