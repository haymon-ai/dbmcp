//! `EMAIL_ADDRESS` recognizer.

use crate::recognizers::prelude::*;

/// Context keywords for email.
const CONTEXT: &[&str] = &["email"];

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
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, email};

    #[test]
    fn carries_context_list() {
        assert_eq!(email().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        email()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_email() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("info@haymon.ai", &[("info@haymon.ai", 0.5)]),
            ("my email address is info@haymon.ai", &[("info@haymon.ai", 0.5)]),
            (
                "try one of these emails: info@haymon.ai or anotherinfo@haymon.ai",
                &[("info@haymon.ai", 0.5), ("anotherinfo@haymon.ai", 0.5)],
            ),
            ("my email is info@haymon.", &[]),
            ("support+test@example.com", &[("support+test@example.com", 0.5)]),
            ("not.an.email@", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
