//! Phone-number national-format grammar validator.
//!
//! Strips separators, then checks the cleaned digit form against the
//! per-region grammars (E.164, NANP, UK, DE). Rejects every leading-`0`
//! digit run shorter than 11 digits — the false-positive class fixed
//! by issue #147.

use crate::recognizer::ValidationOutcome;

/// Phone-number national-format grammar validator.
///
/// Returns [`ValidationOutcome::Unknown`] for accept (so the recognizer's
/// score stays at `0.4` and does not outrank higher-scored entities such
/// as `NHS_NUMBER`), and [`ValidationOutcome::Invalid`] for reject.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let had_plus = candidate.starts_with('+');

    let mut digits = [0u8; 15];
    let mut len = 0usize;
    for &b in candidate.as_bytes() {
        if b.is_ascii_digit() {
            if len >= digits.len() {
                return ValidationOutcome::Invalid;
            }
            digits[len] = b - b'0';
            len += 1;
        }
    }
    let d = &digits[..len];
    // Strip optional NANP `1` country code so 10-digit local and 11-digit `1NXXNXXXXXX` share one rule.
    let nanp_body = if !had_plus && len == 11 && d[0] == 1 {
        &d[1..]
    } else {
        d
    };

    let accept = match (had_plus, d) {
        // E.164: leading `+`, 8-15 digits.
        (true, _) => (8..=15).contains(&len),
        // NANP: 10-digit local form, area-code first digit in [2-9].
        _ if nanp_body.len() == 10 && nanp_body[0] >= 2 => true,
        // UK / DE national form: 11-13 digits starting `0[1-9]`.
        (false, [0, a, ..]) if (11..=13).contains(&len) => (1..=9).contains(a),
        // UK international form (no `+`): 12-13 digits starting `44` then `0?[1-9]`.
        (false, [4, 4, rest @ ..]) if (12..=13).contains(&len) => {
            let i = usize::from(rest.first() == Some(&0));
            rest.get(i).is_some_and(|x| (1..=9).contains(x))
        }
        // DE international form (no `+`): 9-14 digits starting `49`.
        (false, [4, 9, ..]) => (9..=14).contains(&len),
        _ => false,
    };

    if accept {
        ValidationOutcome::Unknown
    } else {
        ValidationOutcome::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::recognizer::ValidationOutcome;

    fn is_valid(s: &str) -> bool {
        validate(s) != ValidationOutcome::Invalid
    }

    #[test]
    fn issue_147_negatives_rejected() {
        assert!(!is_valid("000-12-3456"));
        assert!(!is_valid("07-1234567"));
        assert!(!is_valid("046 454 287"));
        assert!(!is_valid("01234567"));
    }

    #[test]
    fn issue_147_extra_negatives_rejected() {
        assert!(!is_valid("0461234567"));
    }

    #[test]
    fn e164_accepted() {
        assert!(is_valid("+14155552671"));
        assert!(is_valid("+44 20 7946 0958"));
        assert!(is_valid("+49 30 12345678"));
    }

    #[test]
    fn nanp_local_form_accepted() {
        assert!(is_valid("(415) 555-2671"));
        assert!(is_valid("4155552671"));
        assert!(is_valid("1-415-555-2671"));
    }

    #[test]
    fn uk_national_form_accepted() {
        assert!(is_valid("02012345678"));
        assert!(is_valid("020 7946 0958"));
    }

    #[test]
    fn de_national_form_accepted() {
        assert!(is_valid("030 12345678"));
        assert!(is_valid("0151 12345678"));
    }

    #[test]
    fn too_short_rejected() {
        assert!(!is_valid("12"));
        assert!(!is_valid("+12"));
        assert!(!is_valid("1234567"));
    }

    #[test]
    fn too_long_rejected() {
        assert!(!is_valid("+1234567890123456"));
    }

    #[test]
    fn nanp_invalid_area_code_rejected() {
        assert!(!is_valid("0461234567"));
        assert!(!is_valid("(046) 123-4567"));
    }
}
