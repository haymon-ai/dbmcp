//! `API_KEY` recognizer (5 fixed-format providers + keyword-gated AWS secret).
//!
//! Per FR-204 / clarification 2026-05-06 Q5: five named providers only — AWS
//! access key, AWS secret access key, `GitHub` PAT, Stripe live keys, Google
//! API, `OpenAI`. No generic high-entropy fallback. AWS secret needs a keyword
//! context (the regex alone matches any 40-char base64 string).
//!
//! Two `Rule`s ship from this module so the AWS-secret leg can
//! attach a [`crate::KeywordValidator`] without forcing keyword
//! requirements on the strongly-anchored providers.

use crate::recognizer::{Category, KeywordValidator, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

const AWS_SECRET_KEYWORDS: &[&str] = &["secret", "aws_secret_access_key", "aws_secret"];

/// Build the strongly-anchored `API_KEY` recognizer (AWS access, `GitHub`, Stripe, Google, `OpenAI`).
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn api_key_strong() -> Rule {
    let s = Score::from_static(0.6);
    let patterns = vec![
        Regex::new("AWS access", r"\bAKIA[A-Z2-7]{16}\b", s).expect("AWS access compiles"),
        Regex::new("GitHub PAT", r"\bgh[pousr]_[A-Za-z0-9]{36}\b", s).expect("GitHub PAT compiles"),
        Regex::new("Stripe live", r"\b(?:sk|pk)_live_[A-Za-z0-9]{24,}\b", s).expect("Stripe compiles"),
        Regex::new("Google API", r"\bAIza[0-9A-Za-z_\-]{35}\b", s).expect("Google API compiles"),
        Regex::new("OpenAI", r"\bsk-[A-Za-z0-9]{48}\b", s).expect("OpenAI compiles"),
    ];
    Rule::new(entity::API_KEY, patterns)
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
pub fn api_key_aws_secret() -> Rule {
    let pattern = Regex::new("AWS secret", r"\b[A-Za-z0-9+/]{40}\b", Score::from_static(0.3))
        .expect("AWS secret pattern compiles");
    Rule::new(entity::API_KEY, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("ApiKeyAwsSecretRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(AWS_SECRET_KEYWORDS)))
        .with_category(Category::DigitalIdentity)
}

#[cfg(test)]
mod tests {
    use super::{api_key_aws_secret, api_key_strong};

    fn matches_strong(text: &str) -> Vec<String> {
        api_key_strong()
            .analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    fn matches_secret(text: &str) -> Vec<String> {
        api_key_aws_secret()
            .analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_aws_access_key() {
        assert_eq!(
            matches_strong("aws_access_key_id=AKIAIOSFODNN7EXAMPLE"),
            vec!["AKIAIOSFODNN7EXAMPLE"]
        );
    }

    #[test]
    fn positive_github_pat() {
        assert_eq!(
            matches_strong("GH_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
            vec!["ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"]
        );
    }

    #[test]
    fn positive_stripe_live() {
        assert_eq!(
            matches_strong("STRIPE=sk_live_aBcDeFgHiJkLmNoPqRsTuVwX"),
            vec!["sk_live_aBcDeFgHiJkLmNoPqRsTuVwX"]
        );
    }

    #[test]
    fn positive_google_api() {
        // Google API key: AIza prefix + 35 chars from [A-Za-z0-9_-].
        let body: String = std::iter::repeat_n('A', 35).collect();
        let key = format!("AIza{body}");
        assert!(key.len() == 39, "test fixture must be 39 chars, got {}", key.len());
        let key = key.as_str();
        assert_eq!(matches_strong(&format!("GOOGLE_API_KEY={key}")), vec![key.to_string()]);
    }

    #[test]
    fn positive_openai() {
        // OpenAI: sk- prefix + 48 chars from [A-Za-z0-9].
        let body: String = std::iter::repeat_n('a', 48).collect();
        let key = format!("sk-{body}");
        assert!(key.len() == 51, "test fixture must be 51 chars");
        assert_eq!(matches_strong(&format!("OPENAI={key}")), vec![key.clone()]);
    }

    #[test]
    fn positive_aws_secret_with_keyword() {
        // AWS secret: exactly 40 chars [A-Za-z0-9+/].
        let secret: String = std::iter::repeat_n('A', 40).collect();
        let text = format!("aws_secret_access_key={secret}");
        assert_eq!(matches_secret(&text), vec![secret.clone()]);
    }

    #[test]
    fn negative_aws_secret_no_keyword() {
        // 40-char base64 with no `secret` keyword nearby → keyword-context drops it.
        let body: String = std::iter::repeat_n('B', 40).collect();
        assert!(matches_secret(&body).is_empty());
    }

    #[test]
    fn negative_random_high_entropy_string() {
        // No generic high-entropy fallback. A random 40-char base64 string with
        // no provider prefix and no keyword produces zero matches across both legs.
        let body: String = std::iter::repeat_n('C', 40).collect();
        assert!(matches_strong(&body).is_empty());
        assert!(matches_secret(&body).is_empty());
    }

    #[test]
    fn negative_aws_access_with_non_base32_digit() {
        // AWS access keys are base32 (A-Z, 2-7). `0`, `1`, `8`, `9` MUST NOT match.
        assert!(matches_strong("AKIA0OSFODNN7EXAMPLE").is_empty());
        assert!(matches_strong("AKIAIOSFODNN8EXAMPLE").is_empty());
    }

    #[test]
    fn negative_github_pat_oversized() {
        // Legacy GitHub tokens are exactly 36 alnum chars; 37+ MUST NOT match.
        let body: String = std::iter::repeat_n('a', 37).collect();
        assert!(matches_strong(&format!("GH_TOKEN=ghp_{body}")).is_empty());
    }
}
