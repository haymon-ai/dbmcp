//! `API_KEY` recognizer (5 fixed-format providers + keyword-gated AWS secret).
//!
//! Per FR-204 / clarification 2026-05-06 Q5: five named providers only — AWS
//! access key, AWS secret access key, `GitHub` PAT, Stripe live keys, Google
//! API, `OpenAI`. No generic high-entropy fallback. AWS secret needs a keyword
//! context (the regex alone matches any 40-char base64 string).
//!
//! Two `Recognizer`s ship from this module so the AWS-secret leg can
//! attach a [`crate::KeywordValidator`] without forcing keyword
//! requirements on the strongly-anchored providers.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const AWS_SECRET_KEYWORDS: &[&str] = &["secret", "aws_secret_access_key", "aws_secret"];

/// Build the strongly-anchored `API_KEY` recognizer (AWS access, `GitHub`, Stripe, Google, `OpenAI`).
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn api_key_strong() -> Recognizer {
    let s = Score::from_static(0.6);
    let patterns = vec![
        Pattern::new("AWS access", r"\bAKIA[A-Z2-7]{16}\b", s).expect("AWS access compiles"),
        Pattern::new("GitHub PAT", r"\bgh[pousr]_[A-Za-z0-9]{36}\b", s).expect("GitHub PAT compiles"),
        Pattern::new("Stripe live", r"\b(?:sk|pk)_live_[A-Za-z0-9]{24,}\b", s).expect("Stripe compiles"),
        Pattern::new("Google API", r"\bAIza[0-9A-Za-z_\-]{35}\b", s).expect("Google API compiles"),
        Pattern::new("OpenAI", r"\bsk-[A-Za-z0-9]{48}\b", s).expect("OpenAI compiles"),
    ];
    Recognizer::new(Entity::ApiKey, patterns)
        .expect("non-empty pattern list")
        .with_name("ApiKeyRecognizer")
        .with_category(Category::DigitalIdentity)
}

/// Build the keyword-gated AWS secret-access-key recognizer.
///
/// AWS secrets are 40-char base64 strings — the regex alone is far too weak.
/// Strict keyword-context (`secret`, `aws_secret_access_key`, …) inside ±64
/// chars is required (FR-204 / Q2).
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn api_key_aws_secret() -> Recognizer {
    let pattern = Pattern::new("AWS secret", r"\b[A-Za-z0-9+/]{40}\b", Score::from_static(0.3))
        .expect("AWS secret pattern compiles");
    Recognizer::new(Entity::ApiKey, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("ApiKeyAwsSecretRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(AWS_SECRET_KEYWORDS)))
        .with_category(Category::DigitalIdentity)
}

#[cfg(test)]
mod tests {
    use super::{api_key_aws_secret, api_key_strong};

    fn matches_strong(text: &str) -> Vec<(usize, usize)> {
        api_key_strong()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    fn matches_secret(text: &str) -> Vec<(usize, usize)> {
        api_key_aws_secret()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_api_key_strong() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("aws_access_key_id=AKIAIOSFODNN7EXAMPLE", &[(18, 38)]),
            ("GH_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789", &[(9, 49)]),
            ("STRIPE=sk_live_aBcDeFgHiJkLmNoPqRsTuVwX", &[(7, 39)]),
            ("GOOGLE_API_KEY=AIzaAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", &[(15, 54)]),
            ("OPENAI=sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &[(7, 58)]),
            ("AKIA0OSFODNN7EXAMPLE", &[]),
            ("AKIAIOSFODNN8EXAMPLE", &[]),
            ("GH_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &[]),
            ("CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(
                matches_strong(input),
                expected.to_vec(),
                "input {input:?}: span mismatch"
            );
        }
    }

    #[test]
    fn recognizes_api_key_aws_secret() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            (
                "aws_secret_access_key=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                &[(22, 62)],
            ),
            ("BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(
                matches_secret(input),
                expected.to_vec(),
                "input {input:?}: span mismatch"
            );
        }
    }
}
