//! `BANK_ACCOUNT_US` recognizer.
//!
//! Pure digit run of 8–17 chars; the regex on its own is too broad to be
//! useful, so a keyword-context validator drops any match without a banking
//! keyword nearby. Mirrors Presidio's `UsBankRecognizer` weak score plus
//! context boost.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::{KeywordValidator, Validator};
use crate::{Category, Entity};

const KEYWORDS: &[&str] = &["check", "account", "acct", "bank", "savings", "debit", "checking"];

/// Build the `BANK_ACCOUNT_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn bank_account_us() -> Recognizer {
    let pattern = Pattern::new("US Bank Account", r"\b\d{8,17}\b", Score::from_static(0.05))
        .expect("static US bank account pattern compiles");
    Recognizer::new(Entity::BankAccountUs, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("BankAccountUsRecognizer")
        .with_validator(Validator::Keyword(KeywordValidator::new(KEYWORDS)))
        .with_category(Category::Financial)
}

#[cfg(test)]
mod tests {
    use super::bank_account_us;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        bank_account_us()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_bank_account_us() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("checking account 12345678", &[(17, 25)]),
            ("bank 1234567890123", &[(5, 18)]),
            ("savings acct 9876543210", &[(13, 23)]),
            ("order 12345678", &[]),
            ("account 1234567", &[]),
            ("account 123456789012345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
