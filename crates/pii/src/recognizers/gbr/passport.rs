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
    use super::passport_gbr;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        passport_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_passport_gbr() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("AB1234567", &[(0, 9)]),
            ("XY9876543", &[(0, 9)]),
            ("ab1234567", &[(0, 9)]),
            ("My passport number is CD7654321 and it expires soon", &[(22, 31)]),
            ("Passports: AB1234567 and XY9876543", &[(11, 20), (25, 34)]),
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
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
