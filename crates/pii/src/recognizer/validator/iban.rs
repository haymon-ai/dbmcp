//! IBAN mod-97 checksum validator.

use crate::recognizer::ValidationOutcome;

/// IBAN mod-97 validator. Accepts upper-case input; whitespace stripped before checking.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    // Longest legal IBAN is 34 chars; stack-buffer the cleaned form.
    let mut buf = [0u8; 34];
    let mut len = 0usize;
    for &b in candidate.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if !b.is_ascii() || len == buf.len() {
            return ValidationOutcome::Invalid;
        }
        buf[len] = b.to_ascii_uppercase();
        len += 1;
    }
    if len < 15 {
        return ValidationOutcome::Invalid;
    }
    // Rearranged = tail (positions 4..len) followed by head (positions 0..4).
    let rearranged = buf[4..len].iter().chain(buf[..4].iter()).copied();
    ValidationOutcome::from_bool(mod97(rearranged) == Some(1))
}

fn mod97<I: Iterator<Item = u8>>(bytes: I) -> Option<u32> {
    let mut remainder: u32 = 0;
    for b in bytes {
        if b.is_ascii_digit() {
            remainder = (remainder * 10 + u32::from(b - b'0')) % 97;
        } else if b.is_ascii_uppercase() {
            remainder = (remainder * 100 + u32::from(b - b'A' + 10)) % 97;
        } else {
            return None;
        }
    }
    Some(remainder)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::recognizer::ValidationOutcome;

    #[test]
    fn iban_valid_de() {
        // Wikipedia example
        assert_eq!(validate("DE89 3704 0044 0532 0130 00"), ValidationOutcome::Valid);
    }

    #[test]
    fn iban_invalid_check_digits() {
        assert_eq!(validate("DE00 3704 0044 0532 0130 00"), ValidationOutcome::Invalid);
    }
}
