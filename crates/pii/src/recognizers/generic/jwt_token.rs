//! `JWT_TOKEN` recognizer (header `alg` field validated; signature NOT verified).

use crate::recognizers::prelude::*;

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

    const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature";

    fn results(text: &str) -> Vec<(&str, f32)> {
        jwt_token()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_jwt_token() {
        let bearer = format!("Bearer {JWT}");
        let cases: &[(&str, &[(&str, f32)])] = &[
            (bearer.as_str(), &[(JWT, 1.0)]),
            ("version 1.2.3", &[]),
            ("eyJhbGciOiJIUzI1NiJ9.payload", &[]),
            ("eyJ0eXAiOiJKV1QifQ.payload.sig", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
