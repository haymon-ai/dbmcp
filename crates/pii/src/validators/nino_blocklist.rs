//! UK NINO prefix blocklist validator.

use super::digits::collect_upper_alnum;
use crate::ValidationOutcome;

const NINO_BLOCKED_PREFIXES: &[[u8; 2]] = &[*b"BG", *b"GB", *b"KN", *b"NK", *b"NT", *b"TN", *b"ZZ"];

const fn first_letter_disallowed(b: u8) -> bool {
    matches!(b, b'D' | b'F' | b'I' | b'Q' | b'U' | b'V')
}

const fn second_letter_disallowed(b: u8) -> bool {
    matches!(b, b'D' | b'F' | b'I' | b'O' | b'Q' | b'U' | b'V')
}

/// UK NINO blocklist validator.
///
/// Per HMRC rules: first letter MUST NOT be in `{D, F, I, Q, U, V}`; second
/// letter MUST NOT be in `{D, F, I, O, Q, U, V}`; full prefix MUST NOT be in
/// `{BG, GB, KN, NK, NT, TN, ZZ}`; suffix letter (when present) MUST be in
/// `{A, B, C, D}`.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some((buf, len)) = collect_upper_alnum::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    if len != 8 && len != 9 {
        return ValidationOutcome::Invalid;
    }
    let cleaned = &buf[..len];
    if !cleaned[0].is_ascii_alphabetic() || !cleaned[1].is_ascii_alphabetic() {
        return ValidationOutcome::Invalid;
    }
    if first_letter_disallowed(cleaned[0]) || second_letter_disallowed(cleaned[1]) {
        return ValidationOutcome::Invalid;
    }
    let prefix = [cleaned[0], cleaned[1]];
    if NINO_BLOCKED_PREFIXES.contains(&prefix) {
        return ValidationOutcome::Invalid;
    }
    if !cleaned[2..8].iter().all(u8::is_ascii_digit) {
        return ValidationOutcome::Invalid;
    }
    if len == 9 && !matches!(cleaned[8], b'A' | b'B' | b'C' | b'D') {
        return ValidationOutcome::Invalid;
    }
    ValidationOutcome::Valid
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn rejects_disallowed_first_letter() {
        for prefix in ["DA", "FA", "IA", "QA", "UA", "VA"] {
            let candidate = format!("{prefix}123456C");
            assert_eq!(validate(&candidate), ValidationOutcome::Invalid, "{candidate}");
        }
    }

    #[test]
    fn rejects_disallowed_second_letter() {
        for prefix in ["AD", "AF", "AI", "AQ", "AU", "AV"] {
            let candidate = format!("{prefix}123456C");
            assert_eq!(validate(&candidate), ValidationOutcome::Invalid, "{candidate}");
        }
    }
}
