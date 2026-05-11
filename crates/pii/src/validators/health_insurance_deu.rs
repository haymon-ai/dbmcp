//! German Krankenversicherungsnummer (KVNR) checksum validator (GKV-Spitzenverband, § 290 SGB V).
//!
//! Format: one uppercase letter (birth surname initial) followed by nine
//! digits, last of which is the check digit. The letter is expanded to its
//! two-digit ordinal (`A=01 .. Z=26`); alternating weights
//! `[1, 2, 1, 2, 1, 2, 1, 2, 1, 2]` are applied to the ten resulting digits
//! plus the first eight body digits; products ≥10 are Quersumme-collapsed;
//! the sum modulo 10 must equal the trailing check digit.

use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let trimmed = candidate.trim();
    if trimmed.len() != 10 {
        return ValidationOutcome::Invalid;
    }
    let bytes = trimmed.as_bytes();
    let head = bytes[0].to_ascii_uppercase();
    if !head.is_ascii_uppercase() {
        return ValidationOutcome::Invalid;
    }
    if !bytes[1..].iter().all(u8::is_ascii_digit) {
        return ValidationOutcome::Invalid;
    }
    let letter_ord = u32::from(head - b'A') + 1;
    let mut effective = [0u32; 10];
    effective[0] = letter_ord / 10;
    effective[1] = letter_ord % 10;
    for (i, b) in bytes[1..9].iter().copied().enumerate() {
        effective[i + 2] = u32::from(b - b'0');
    }
    let check = u32::from(bytes[9] - b'0');
    let weights = [1u32, 2, 1, 2, 1, 2, 1, 2, 1, 2];
    let mut total: u32 = 0;
    for (digit, weight) in effective.iter().copied().zip(weights) {
        let product = digit * weight;
        total += if product >= 10 {
            (product / 10) + (product % 10)
        } else {
            product
        };
    }
    ValidationOutcome::from_bool(total % 10 == check)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn official_examples_pass() {
        assert_eq!(validate("A000500015"), ValidationOutcome::Valid);
        assert_eq!(validate("C000500021"), ValidationOutcome::Valid);
    }

    #[test]
    fn computed_examples_pass() {
        assert_eq!(validate("A123456780"), ValidationOutcome::Valid);
        assert_eq!(validate("B123456782"), ValidationOutcome::Valid);
        assert_eq!(validate("M123456785"), ValidationOutcome::Valid);
        assert_eq!(validate("Z000000005"), ValidationOutcome::Valid);
        assert_eq!(validate("Z999999997"), ValidationOutcome::Valid);
    }

    #[test]
    fn lowercase_normalized() {
        assert_eq!(validate("a123456780"), ValidationOutcome::Valid);
    }

    #[test]
    fn wrong_check_digit_rejected() {
        assert_eq!(validate("A123456787"), ValidationOutcome::Invalid);
        assert_eq!(validate("M123456789"), ValidationOutcome::Invalid);
        assert_eq!(validate("A000500010"), ValidationOutcome::Invalid);
    }

    #[test]
    fn leading_digit_rejected() {
        assert_eq!(validate("1123456780"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("A12345678"), ValidationOutcome::Invalid);
        assert_eq!(validate("A1234567890"), ValidationOutcome::Invalid);
    }
}
