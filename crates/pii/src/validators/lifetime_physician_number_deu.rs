//! German lifetime physician number (Lebenslange Arztnummer / LANR) checksum validator (KBV spec).
//!
//! Nine digits total. Weights `[4, 9, 4, 9, 4, 9]` are applied to the first
//! six digits; the seventh digit must equal `(10 - sum mod 10) mod 10`.
//! Digits 8 and 9 encode the medical specialty and are not part of the
//! checksum.

use super::digits::collect_digits;
use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let weights = [4u32, 9, 4, 9, 4, 9];
    let sum: u32 = digits[..6].iter().zip(weights).map(|(d, w)| d * w).sum();
    let expected = (10 - sum % 10) % 10;
    ValidationOutcome::from_bool(digits[6] == expected)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn valid_lanr_passes() {
        assert_eq!(validate("123456601"), ValidationOutcome::Valid);
        assert_eq!(validate("234567701"), ValidationOutcome::Valid);
        assert_eq!(validate("100000601"), ValidationOutcome::Valid);
        assert_eq!(validate("987654401"), ValidationOutcome::Valid);
        assert_eq!(validate("555555501"), ValidationOutcome::Valid);
        assert_eq!(validate("999999901"), ValidationOutcome::Valid);
    }

    #[test]
    fn wrong_check_digit_rejected() {
        assert_eq!(validate("123456901"), ValidationOutcome::Invalid);
        assert_eq!(validate("234567601"), ValidationOutcome::Invalid);
        assert_eq!(validate("100000401"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("12345660"), ValidationOutcome::Invalid);
        assert_eq!(validate("1234566010"), ValidationOutcome::Invalid);
    }
}
