//! `VAT_NUMBER` recognizer (EU / UK / Northern Ireland VAT identifier).

use crate::recognizer::{Category, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `VAT_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn vat_number() -> Rule {
    let pattern = Regex::new(
        "VAT (ISO2 + body)",
        r"\b[A-Z]{2}[A-Z0-9]{7,12}\b",
        Score::from_static(0.4),
    )
    .expect("static VAT pattern compiles");
    Rule::new(entity::VAT_NUMBER, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("VatNumberRecognizer")
        .with_validator(Validator::VatCountryLength)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::vat_number;

    fn matches(text: &str) -> Vec<String> {
        let r = vat_number();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_de_length_9() {
        assert_eq!(matches("VAT DE123456789"), vec!["DE123456789"]);
    }

    #[test]
    fn positive_gb_length_9() {
        assert_eq!(matches("VAT GB123456789"), vec!["GB123456789"]);
    }

    #[test]
    fn unknown_prefix_rejected() {
        // Stops uppercase-word false positives like CERTIFICATE, DEMOGRAPHIC.
        assert!(matches("VAT XX123456789").is_empty());
    }

    #[test]
    fn negative_de_too_short() {
        // DE is exactly 9 — fewer body digits → invalid.
        assert!(matches("DE12345").is_empty());
    }
}
