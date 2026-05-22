//! `PASSPORT_UK` recognizer (post-2015 format: 2 letters + 7 digits).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for UK passport.
const CONTEXT: &[&str] = &[
    "passport",
    "passport number",
    "travel document",
    "uk passport",
    "british passport",
    "her majesty",
    "his majesty",
    "hm passport",
    "hmpo",
];

/// Build the `PASSPORT_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn passport_gbr() -> Recognizer {
    let pattern = Pattern::new("UK Passport (weak)", r"(?i)\b[A-Z]{2}\d{7}\b", Score::from_static(0.1))
        .expect("static UK passport pattern compiles");
    Recognizer::new(Entity::PassportUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PassportGbrRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, passport_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(passport_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        passport_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_passport_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("AB1234567", &[("AB1234567", 0.1)]),
            ("XY9876543", &[("XY9876543", 0.1)]),
            ("ab1234567", &[("ab1234567", 0.1)]),
            (
                "My passport number is CD7654321 and it expires soon",
                &[("CD7654321", 0.1)],
            ),
            (
                "Passports: AB1234567 and XY9876543",
                &[("AB1234567", 0.1), ("XY9876543", 0.1)],
            ),
            ("A12345678", &[]),
            ("ABC123456", &[]),
            ("AB123456", &[]),
            ("AB12345678", &[]),
            ("123456789", &[]),
            ("AB 1234567", &[]),
            ("1234567AB", &[]),
            ("XYZAB1234567QRS", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
