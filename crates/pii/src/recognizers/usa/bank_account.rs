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
    use super::{CONTEXT, bank_account_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(bank_account_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        bank_account_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_bank_account_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("checking account 12345678", &[("12345678", 0.05)]),
            ("bank 1234567890123", &[("1234567890123", 0.05)]),
            ("savings acct 9876543210", &[("9876543210", 0.05)]),
            ("order 12345678", &[("12345678", 0.05)]),
            ("account 1234567", &[]),
            ("account 123456789012345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
