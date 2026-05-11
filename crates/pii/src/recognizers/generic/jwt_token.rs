//! `JWT_TOKEN` recognizer (header `alg` field validated; signature NOT verified).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `JWT_TOKEN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn jwt_token() -> Recognizer {
    let pattern = Pattern::new(
        "JWT (3 base64url segments)",
        r"\b[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
        Score::from_static(0.3),
    )
    .expect("static JWT pattern compiles");
    Recognizer::new(Entity::JwtToken, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("JwtTokenRecognizer")
        .with_validator(Validator::JwtHeader)
        .with_category(Category::DigitalIdentity)
}

#[cfg(test)]
mod tests {
    use super::jwt_token;

    fn matches(text: &str) -> Vec<String> {
        jwt_token()
            .analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_real_header_with_alg() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature";
        assert_eq!(matches(&format!("Bearer {jwt}")), vec![jwt.to_string()]);
    }

    #[test]
    fn negative_dotted_version_string() {
        assert!(matches("version 1.2.3").is_empty());
    }

    #[test]
    fn negative_two_segments() {
        let body = "eyJhbGciOiJIUzI1NiJ9.payload";
        assert!(matches(body).is_empty());
    }

    #[test]
    fn negative_header_without_alg() {
        let bad = "eyJ0eXAiOiJKV1QifQ.payload.sig";
        assert!(matches(bad).is_empty());
    }
}
