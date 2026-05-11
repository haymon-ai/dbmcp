//! ICAO Doc 9303 9-character MRZ check-digit validator.
//!
//! Used by the German passport recognizer. Weights `[7, 3, 1]` repeat across
//! the first eight characters; letters map `A=10 .. Z=35`; the sum modulo 10
//! must equal the trailing digit. Visually ambiguous letters `A B D E I O Q S U`
//! are rejected outright per ICAO Doc 9303 §3.

use crate::ValidationOutcome;

const FORBIDDEN: &[u8] = b"ABDEIOQSU";

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let trimmed = candidate.trim();
    if trimmed.len() != 9 {
        return ValidationOutcome::Invalid;
    }
    let bytes = trimmed.as_bytes();
    let last = bytes[8];
    if !last.is_ascii_digit() {
        return ValidationOutcome::Invalid;
    }
    let mut total: u32 = 0;
    let weights = [7u32, 3, 1];
    for (i, b) in bytes[..8].iter().copied().enumerate() {
        let upper = b.to_ascii_uppercase();
        let value = if upper.is_ascii_digit() {
            u32::from(upper - b'0')
        } else if upper.is_ascii_uppercase() {
            if FORBIDDEN.contains(&upper) {
                return ValidationOutcome::Invalid;
            }
            u32::from(upper - b'A') + 10
        } else {
            return ValidationOutcome::Invalid;
        };
        total += value * weights[i % 3];
    }
    ValidationOutcome::from_bool(total % 10 == u32::from(last - b'0'))
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn valid_passport_passes() {
        assert_eq!(validate("C01234565"), ValidationOutcome::Valid);
        assert_eq!(validate("F12345671"), ValidationOutcome::Valid);
        assert_eq!(validate("L01X00T44"), ValidationOutcome::Valid);
        assert_eq!(validate("CZ6311T03"), ValidationOutcome::Valid);
        assert_eq!(validate("G00000002"), ValidationOutcome::Valid);
        assert_eq!(validate("C01X00T41"), ValidationOutcome::Valid);
    }

    #[test]
    fn lowercase_normalized() {
        assert_eq!(validate("c01234565"), ValidationOutcome::Valid);
    }

    #[test]
    fn bad_checksum_rejected() {
        assert_eq!(validate("C01234567"), ValidationOutcome::Invalid);
        assert_eq!(validate("F12345678"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("C0123456"), ValidationOutcome::Invalid);
        assert_eq!(validate("C012345678"), ValidationOutcome::Invalid);
    }

    #[test]
    fn forbidden_letter_rejected() {
        assert_eq!(validate("A01234567"), ValidationOutcome::Invalid);
        assert_eq!(validate("IOQSUBDE1"), ValidationOutcome::Invalid);
    }
}
