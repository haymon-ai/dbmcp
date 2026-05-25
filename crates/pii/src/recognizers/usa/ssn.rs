//! `US_SSN` recognizer.
//!
//! Plain regex matches `XXX-XX-XXXX` shape; reserved area/group/serial values
//! are rejected by [`UsSsnValidator`].

use crate::recognizers::prelude::*;

/// Context keywords for US SSN.
const CONTEXT: &[&str] = &["social", "security", "ssn", "ssns", "ssid"];

/// Build the `US_SSN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn ssn_usa() -> Recognizer {
    let pattern = Pattern::new("US SSN", r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b", Score::from_static(0.6))
        .expect("static SSN pattern compiles");
    Recognizer::new(Entity::UsSsn, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SsnUsaRecognizer")
        .with_validator(Validator::SsnUsa)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, ssn_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(ssn_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        ssn_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_ssn_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("078-051121 07805-1121", &[("078-051121", 1.0), ("07805-1121", 1.0)]),
            ("078051121", &[("078051121", 1.0)]),
            ("078-05-1123", &[("078-05-1123", 1.0)]),
            ("078 05 1123", &[("078 05 1123", 1.0)]),
            ("abc 078 05 1123 abc", &[("078 05 1123", 1.0)]),
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
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
