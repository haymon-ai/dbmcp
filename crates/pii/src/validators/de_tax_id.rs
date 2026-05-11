//! German Steueridentifikationsnummer (Steuer-IdNr.) checksum validator.
//!
//! Eleven digits, leading digit `1-9`. The Bundeszentralamt für Steuern's
//! post-2016 rule additionally forbids any digit appearing more than three
//! times in positions 1-10. The checksum is ISO 7064 Mod 11, 10 over the
//! first ten digits.

use super::digits::collect_digits;
use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<11>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    if digits[0] == 0 {
        return ValidationOutcome::Invalid;
    }
    let mut histogram = [0u32; 10];
    for d in &digits[..10] {
        histogram[*d as usize] += 1;
    }
    if histogram.iter().any(|&c| c > 3) {
        return ValidationOutcome::Invalid;
    }
    let mut product: u32 = 10;
    for d in &digits[..10] {
        let mut total = (d + product) % 10;
        if total == 0 {
            total = 10;
        }
        product = (total * 2) % 11;
    }
    let check = if 11 - product == 10 { 0 } else { 11 - product };
    ValidationOutcome::from_bool(check == digits[10])
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn valid_tax_id_passes() {
        assert_eq!(validate("12345678903"), ValidationOutcome::Valid);
        assert_eq!(validate("98765432106"), ValidationOutcome::Valid);
    }

    #[test]
    fn wrong_check_digit_rejected() {
        assert_eq!(validate("12345678901"), ValidationOutcome::Invalid);
        assert_eq!(validate("98765432100"), ValidationOutcome::Invalid);
    }

    #[test]
    fn leading_zero_rejected() {
        assert_eq!(validate("02345678903"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("1234567890"), ValidationOutcome::Invalid);
        assert_eq!(validate("123456789030"), ValidationOutcome::Invalid);
    }

    #[test]
    fn excess_repetition_rejected() {
        assert_eq!(validate("11111111111"), ValidationOutcome::Invalid);
        assert_eq!(validate("11112345678"), ValidationOutcome::Invalid);
        assert_eq!(validate("12222234567"), ValidationOutcome::Invalid);
    }
}
