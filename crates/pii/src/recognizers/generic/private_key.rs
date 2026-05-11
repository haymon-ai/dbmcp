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

    fn matches(text: &str) -> Vec<String> {
        private_key()
            .analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_rsa() {
        let block = "-----BEGIN RSA PRIVATE KEY-----\n\
                     MIIEowIBAAKCAQEAfake==\n\
                     -----END RSA PRIVATE KEY-----";
        assert_eq!(matches(block), vec![block.to_string()]);
    }

    #[test]
    fn positive_ec() {
        let block = "-----BEGIN EC PRIVATE KEY-----\n\
                     MHcCAQEEIfake==\n\
                     -----END EC PRIVATE KEY-----";
        assert_eq!(matches(block), vec![block.to_string()]);
    }

    #[test]
    fn positive_openssh() {
        let block = "-----BEGIN OPENSSH PRIVATE KEY-----\nbase64data\n-----END OPENSSH PRIVATE KEY-----";
        assert_eq!(matches(block), vec![block.to_string()]);
    }

    #[test]
    fn negative_certificate_block() {
        let cert = "-----BEGIN CERTIFICATE-----\nbase64\n-----END CERTIFICATE-----";
        assert!(matches(cert).is_empty());
    }

    #[test]
    fn negative_mismatched_types() {
        // Regex matches BEGIN..END pair; PrivateKeyTypeValidator rejects type mismatch.
        let bad = "-----BEGIN RSA PRIVATE KEY-----\nbase64\n-----END EC PRIVATE KEY-----";
        assert!(matches(bad).is_empty(), "type mismatch must drop");
    }
}
