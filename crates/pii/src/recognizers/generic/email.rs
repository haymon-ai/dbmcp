//! `EMAIL_ADDRESS` recognizer.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `EMAIL_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction;
/// both are unit-tested.
#[must_use]
pub fn email() -> Recognizer {
    let pattern = Pattern::new(
        "Email (Medium)",
        r"\b[A-Za-z0-9!#$%&'*+\-/=?^_`{|}~]+(?:\.[A-Za-z0-9!#$%&'*+\-/=?^_`{|}~]+)*@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+\b",
        Score::from_static(0.5),
    )
    .expect("static email pattern compiles");
    Recognizer::new(Entity::EmailAddress, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("EmailRecognizer")
        .with_category(Category::Personal)
}

#[cfg(test)]
mod tests {
    use super::email;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        email().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_email() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("info@haymon.ai", &[(0, 14)]),
            ("my email address is info@haymon.ai", &[(20, 34)]),
            (
                "try one of these emails: info@haymon.ai or anotherinfo@haymon.ai",
                &[(25, 39), (43, 64)],
            ),
            ("my email is info@haymon.", &[]),
            ("support+test@example.com", &[(0, 24)]),
            ("not.an.email@", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
