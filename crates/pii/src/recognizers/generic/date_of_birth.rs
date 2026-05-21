//! `DATE_OF_BIRTH` recognizer (weak date pattern, keyword-context gated).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for date of birth (English + German).
const CONTEXT: &[&str] = &[
    "birth",
    "dob",
    "born",
    "birthday",
    "birthdate",
    "dateofbirth",
    "geburtsdatum",
    "geburtstag",
    "geboren",
    "geburt",
];

/// Build the `DATE_OF_BIRTH` recognizer.
///
/// # Panics
///
/// Panics only if a bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn date_of_birth() -> Recognizer {
    let iso = Pattern::new(
        "ISO date (YYYY-MM-DD)",
        r"\b\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])\b",
        Score::from_static(0.05),
    )
    .expect("static ISO date pattern compiles");
    let dmy = Pattern::new(
        "DMY date (DD.MM.YYYY / DD/MM/YYYY)",
        r"\b(0[1-9]|[12]\d|3[01])[./](0[1-9]|1[0-2])[./]\d{4}\b",
        Score::from_static(0.05),
    )
    .expect("static DMY date pattern compiles");
    let mdy = Pattern::new(
        "MDY date (MM/DD/YYYY)",
        r"\b(0[1-9]|1[0-2])/(0[1-9]|[12]\d|3[01])/\d{4}\b",
        Score::from_static(0.05),
    )
    .expect("static MDY date pattern compiles");
    Recognizer::new(Entity::DateOfBirth, vec![iso, dmy, mdy])
        .expect("non-empty pattern list")
        .with_name("DateOfBirthRecognizer")
        .with_category(Category::Personal)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, date_of_birth};

    #[test]
    fn carries_context_list() {
        assert_eq!(date_of_birth().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        date_of_birth()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_date_of_birth() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("1985-07-23", &[("1985-07-23", 0.05)]),
            ("23.07.1985", &[("23.07.1985", 0.05)]),
            ("23/07/1985", &[("23/07/1985", 0.05)]),
            ("07/23/1985", &[("07/23/1985", 0.05)]),
            ("born 2021-08-11", &[("2021-08-11", 0.05)]),
            ("1985-13-23", &[]),
            ("1985-07-32", &[]),
            ("32.07.1985", &[]),
            ("hello world", &[]),
            ("12345", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
