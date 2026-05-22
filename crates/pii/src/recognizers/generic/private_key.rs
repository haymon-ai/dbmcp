//! `PRIVATE_KEY` recognizer (PEM-fenced block; BEGIN-type == END-type).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `PRIVATE_KEY` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn private_key() -> Recognizer {
    let pattern = Pattern::new(
        "PEM private key block",
        r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
        Score::from_static(0.6),
    )
    .expect("static PEM pattern compiles");
    Recognizer::new(Entity::PrivateKey, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PrivateKeyRecognizer")
        .with_validator(Validator::PrivateKeyType)
        .with_category(Category::DigitalIdentity)
}

#[cfg(test)]
mod tests {
    use super::private_key;

    const RSA: &str = "-----BEGIN RSA PRIVATE KEY-----\n\
                       MIIEowIBAAKCAQEAfake==\n\
                       -----END RSA PRIVATE KEY-----";
    const EC: &str = "-----BEGIN EC PRIVATE KEY-----\n\
                      MHcCAQEEIfake==\n\
                      -----END EC PRIVATE KEY-----";
    const OPENSSH: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nbase64data\n-----END OPENSSH PRIVATE KEY-----";
    const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nbase64\n-----END CERTIFICATE-----";
    const MISMATCHED: &str = "-----BEGIN RSA PRIVATE KEY-----\nbase64\n-----END EC PRIVATE KEY-----";

    fn results(text: &str) -> Vec<(&str, f32)> {
        private_key()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_private_key() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            (RSA, &[(RSA, 1.0)]),
            (EC, &[(EC, 1.0)]),
            (OPENSSH, &[(OPENSSH, 1.0)]),
            (CERTIFICATE, &[]),
            (MISMATCHED, &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
