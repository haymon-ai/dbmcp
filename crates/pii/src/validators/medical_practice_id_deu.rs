//! German medical practice ID (Betriebsstättennummer / BSNR) structural validator.
//!
//! No published checksum exists. The validator enforces the structural
//! invariants documented by the Kassenärztliche Bundesvereinigung: exactly
//! nine digits and not the all-zero placeholder.

use super::digits::collect_digits;
use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    if digits.iter().all(|d| *d == 0) {
        return ValidationOutcome::Invalid;
    }
    ValidationOutcome::Unknown
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn nine_digits_pass() {
        assert_eq!(validate("021234568"), ValidationOutcome::Unknown);
        assert_eq!(validate("991234567"), ValidationOutcome::Unknown);
    }

    #[test]
    fn all_zero_rejected() {
        assert_eq!(validate("000000000"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("02123456"), ValidationOutcome::Invalid);
        assert_eq!(validate("0212345689"), ValidationOutcome::Invalid);
    }
}
