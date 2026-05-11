//! German Rentenversicherungsnummer (RVNR) checksum validator (VKVV § 4).
//!
//! Twelve characters: eight digits + one uppercase letter + three digits.
//! Positions 3-4 encode the birth day (01-31 or 51-81 with the
//! Ergänzungsmerkmal offset); positions 5-6 the birth month (01-12). The
//! letter at position 9 is expanded to its two-digit ordinal (`A=01..Z=26`);
//! weights `[2, 1, 2, 5, 7, 1, 2, 1, 2, 1, 2, 1]` are applied to the twelve
//! resulting digits; each product is Quersumme-collapsed; sum modulo 10
//! must equal the trailing check digit.

use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let trimmed = candidate.trim();
    if trimmed.len() != 12 {
        return ValidationOutcome::Invalid;
    }
    let bytes = trimmed.as_bytes();
    if !bytes[..8].iter().all(u8::is_ascii_digit) || !bytes[9..].iter().all(u8::is_ascii_digit) {
        return ValidationOutcome::Invalid;
    }
    let letter = bytes[8].to_ascii_uppercase();
    if !letter.is_ascii_uppercase() {
        return ValidationOutcome::Invalid;
    }
    let day = u32::from(bytes[2] - b'0') * 10 + u32::from(bytes[3] - b'0');
    let month = u32::from(bytes[4] - b'0') * 10 + u32::from(bytes[5] - b'0');
    if !((1..=31).contains(&day) || (51..=81).contains(&day)) {
        return ValidationOutcome::Invalid;
    }
    if !(1..=12).contains(&month) {
        return ValidationOutcome::Invalid;
    }
    let letter_ord = u32::from(letter - b'A') + 1;
    let mut effective = [0u32; 12];
    for (i, b) in bytes[..8].iter().copied().enumerate() {
        effective[i] = u32::from(b - b'0');
    }
    effective[8] = letter_ord / 10;
    effective[9] = letter_ord % 10;
    effective[10] = u32::from(bytes[9] - b'0');
    effective[11] = u32::from(bytes[10] - b'0');
    let check = u32::from(bytes[11] - b'0');
    let weights = [2u32, 1, 2, 5, 7, 1, 2, 1, 2, 1, 2, 1];
    let mut total: u32 = 0;
    for (digit, weight) in effective.iter().copied().zip(weights) {
        let product = digit * weight;
        total += (product / 10) + (product % 10);
    }
    ValidationOutcome::from_bool(total % 10 == check)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn canonical_examples_pass() {
        assert_eq!(validate("15070649C103"), ValidationOutcome::Valid);
        assert_eq!(validate("65070803A019"), ValidationOutcome::Valid);
        assert_eq!(validate("20151090B023"), ValidationOutcome::Valid);
        assert_eq!(validate("38551285K051"), ValidationOutcome::Valid);
    }

    #[test]
    fn wrong_check_digit_rejected() {
        assert_eq!(validate("15070649C100"), ValidationOutcome::Invalid);
        assert_eq!(validate("65070803A012"), ValidationOutcome::Invalid);
    }

    #[test]
    fn invalid_day_or_month_rejected() {
        assert_eq!(validate("15070049C103"), ValidationOutcome::Invalid);
        assert_eq!(validate("15071349C103"), ValidationOutcome::Invalid);
        assert_eq!(validate("15420649C103"), ValidationOutcome::Invalid);
        assert_eq!(validate("15850649C103"), ValidationOutcome::Invalid);
    }

    #[test]
    fn non_letter_at_position_nine_rejected() {
        assert_eq!(validate("150706491103"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("15070649C10"), ValidationOutcome::Invalid);
    }
}
