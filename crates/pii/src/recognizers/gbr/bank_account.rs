//! `BANK_ACCOUNT_UK` recognizer (keyword-context required).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `BANK_ACCOUNT_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn bank_account_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK bank account (8-10 digits)",
        r"\b\d{8,10}\b",
        Score::from_static(0.4),
    )
    .expect("static UK bank-account pattern compiles");
    Recognizer::new(Entity::BankAccountUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("BankAccountGbrRecognizer")
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::bank_account_gbr;

    fn matches(text: &str) -> Vec<String> {
        let r = bank_account_gbr();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_with_keyword() {
        assert_eq!(matches("acct 12345678"), vec!["12345678"]);
    }

    #[test]
    fn positive_iban_keyword() {
        assert_eq!(matches("IBAN account 12345678"), vec!["12345678"]);
    }

    #[test]
    fn negative_too_short() {
        assert!(matches("account 1234567").is_empty());
    }
}
