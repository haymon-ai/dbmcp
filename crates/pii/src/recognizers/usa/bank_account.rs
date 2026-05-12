//! `BANK_ACCOUNT_US` recognizer.
//!
//! Pure digit run of 8–17 chars; the regex on its own is too broad to be
//! useful. Weak base score paired with context keywords: the context-aware
//! scoring pass lifts matches whose surrounding window or owning JSON key
//! contains a banking keyword. Matches without a nearby keyword fall below
//! the redactor's `min_score` floor and are dropped.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for US bank account.
const CONTEXT: &[&str] = &["check", "account", "acct", "bank", "save", "debit"];

/// Build the `BANK_ACCOUNT_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn bank_account_usa() -> Recognizer {
    let pattern = Pattern::new("US Bank Account", r"\b\d{8,17}\b", Score::from_static(0.05))
        .expect("static US bank account pattern compiles");
    Recognizer::new(Entity::BankAccountUs, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("BankAccountUsaRecognizer")
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::bank_account_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        bank_account_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_bank_account_usa() {
        // The regex matches any 8-17 digit run; context-boost + redactor
        // `min_score` floor decide whether the match surfaces.
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("checking account 12345678", &[(17, 25)]),
            ("bank 1234567890123", &[(5, 18)]),
            ("savings acct 9876543210", &[(13, 23)]),
            ("order 12345678", &[(6, 14)]),
            ("account 1234567", &[]),
            ("account 123456789012345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
