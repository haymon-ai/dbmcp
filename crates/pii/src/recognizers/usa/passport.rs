//! `PASSPORT_US` recognizer (9-digit weak + Next Generation [letter + 8 digits]).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for US passport.
const CONTEXT: &[&str] = &["us", "united", "states", "passport", "travel", "document"];

/// Build the `PASSPORT_US` recognizer.
///
/// # Panics
///
/// Panics only if either bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn passport_usa() -> Recognizer {
    let patterns = vec![
        Pattern::new("Passport (very weak)", r"\b\d{9}\b", Score::from_static(0.05))
            .expect("static US passport (9-digit) pattern compiles"),
        Pattern::new(
            "Passport Next Generation (very weak)",
            r"(?i)\b[A-Z]\d{8}\b",
            Score::from_static(0.1),
        )
        .expect("static US passport (next-gen) pattern compiles"),
    ];
    Recognizer::new(Entity::PassportUs, patterns)
        .expect("non-empty pattern list")
        .with_name("PassportUsaRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::passport_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        passport_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_passport_usa() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("912803456", &[(0, 9)]),
            ("Z12803456", &[(0, 9)]),
            ("A12803456", &[(0, 9)]),
            ("my travel document is A12803456", &[(22, 31)]),
            ("my travel passport is A12803456", &[(22, 31)]),
            ("12345678", &[]),
            ("1234567890", &[]),
            ("AB12803456", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
