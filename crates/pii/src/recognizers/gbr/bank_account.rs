//! `BANK_ACCOUNT_UK` recognizer (weak digit pattern, keyword-context gated).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for UK bank account.
const CONTEXT: &[&str] = &["account", "acct", "bank", "iban"];

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
        Score::from_static(0.05),
    )
    .expect("static UK bank-account pattern compiles");
    Recognizer::new(Entity::BankAccountUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("BankAccountGbrRecognizer")
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, bank_account_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(bank_account_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        bank_account_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_bank_account_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("acct 12345678", &[("12345678", 0.05)]),
            ("IBAN account 12345678", &[("12345678", 0.05)]),
            ("1234567890", &[("1234567890", 0.05)]),
            ("900000000", &[("900000000", 0.05)]),
            ("44455512", &[("44455512", 0.05)]),
            ("account 1234567", &[]),
            ("12345678901", &[]),
            ("123456", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
