//! German Personalausweis (nPA) ICAO check-digit validator.
//!
//! The nPA serial uses the ICAO Doc 9303 weighted sum (weights `[7, 3, 1]`,
//! letters `A=10..Z=35`, sum mod 10 must equal the trailing digit). The
//! recognizer also accepts the legacy `T` + eight-digit format that predates
//! 2010 and carries no check digit — for that branch the validator returns
//! [`ValidationOutcome::Unknown`] so the pattern score is preserved.

use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let trimmed = candidate.trim();
    if trimmed.len() != 9 {
        return ValidationOutcome::Invalid;
    }
    let bytes = trimmed.as_bytes();
    if bytes[0].eq_ignore_ascii_case(&b'T') && bytes[1..].iter().all(u8::is_ascii_digit) {
        return ValidationOutcome::Unknown;
    }
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
    fn npa_valid_passes() {
        assert_eq!(validate("L01X00T44"), ValidationOutcome::Valid);
        assert_eq!(validate("C01234565"), ValidationOutcome::Valid);
        assert_eq!(validate("CZ6311T03"), ValidationOutcome::Valid);
        assert_eq!(validate("G00000002"), ValidationOutcome::Valid);
    }

    #[test]
    fn npa_invalid_rejected() {
        assert_eq!(validate("L01X00T47"), ValidationOutcome::Invalid);
        assert_eq!(validate("C01234567"), ValidationOutcome::Invalid);
    }

    #[test]
    fn legacy_t_format_unknown() {
        assert_eq!(validate("T22000129"), ValidationOutcome::Unknown);
        assert_eq!(validate("T00000000"), ValidationOutcome::Unknown);
        assert_eq!(validate("T99999999"), ValidationOutcome::Unknown);
    }

    #[test]
    fn lowercase_normalized() {
        assert_eq!(validate("l01x00t44"), ValidationOutcome::Valid);
        assert_eq!(validate("t22000129"), ValidationOutcome::Unknown);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("T2200012"), ValidationOutcome::Invalid);
        assert_eq!(validate("T220001290"), ValidationOutcome::Invalid);
    }
}
