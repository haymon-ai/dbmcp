//! `US_SSN` recognizer.
//!
//! Plain regex matches `XXX-XX-XXXX` shape; reserved area/group/serial values
//! are rejected by [`UsSsnValidator`].

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `US_SSN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn us_ssn() -> Recognizer {
    let pattern = Pattern::new("US SSN", r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b", Score::from_static(0.6))
        .expect("static SSN pattern compiles");
    Recognizer::new(Entity::UsSsn, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("UsSsnRecognizer")
        .with_validator(Validator::UsSsn)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::us_ssn;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        us_ssn().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_us_ssn() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("078-051121 07805-1121", &[(0, 10), (11, 21)]),
            ("078051121", &[(0, 9)]),
            ("078-05-1123", &[(0, 11)]),
            ("078 05 1123", &[(0, 11)]),
            ("abc 078 05 1123 abc", &[(4, 15)]),
            ("0780511201", &[]),
            ("000000000", &[]),
            ("666000000", &[]),
            ("912-12-1234", &[]),
            ("078-05-0000", &[]),
            ("078 00 1123", &[]),
            ("693-09.4444", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
