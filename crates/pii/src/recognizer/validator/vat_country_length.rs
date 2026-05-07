//! EU / UK VAT-number country-length validator.

use super::digits::collect_upper_alnum;
use crate::recognizer::ValidationOutcome;

const VAT_COUNTRY_LENGTHS: &[([u8; 2], u32, u32)] = &[
    (*b"AT", 9, 9),   // U + 8 digits
    (*b"BE", 10, 10), // 10 digits
    (*b"BG", 9, 10),
    (*b"CY", 9, 9),
    (*b"CZ", 8, 10),
    (*b"DE", 9, 9),
    (*b"DK", 8, 8),
    (*b"EE", 9, 9),
    (*b"EL", 9, 9), // Greece (alt code)
    (*b"GR", 9, 9),
    (*b"ES", 9, 9),
    (*b"FI", 8, 8),
    (*b"FR", 11, 11),
    (*b"GB", 9, 12), // 9 short, 12 long
    (*b"HR", 11, 11),
    (*b"HU", 8, 8),
    (*b"IE", 8, 9),
    (*b"IT", 11, 11),
    (*b"LT", 9, 12),
    (*b"LU", 8, 8),
    (*b"LV", 11, 11),
    (*b"MT", 8, 8),
    (*b"NL", 12, 12),
    (*b"PL", 10, 10),
    (*b"PT", 9, 9),
    (*b"RO", 2, 10),
    (*b"SE", 12, 12),
    (*b"SI", 8, 8),
    (*b"SK", 10, 10),
    (*b"XI", 9, 12), // Northern Ireland post-Brexit
];

/// EU / UK VAT-number country-length validator.
///
/// Format `<ISO2><alphanumeric>`. Checks the alphanumeric body length against
/// a per-country window. Unknown prefix → [`ValidationOutcome::Unknown`] so
/// niche/new countries are not over-rejected.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    // ISO2 prefix + up to 12-char body fits in 14 bytes.
    let Some((buf, len)) = collect_upper_alnum::<14>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    if len < 3 {
        return ValidationOutcome::Invalid;
    }
    let prefix = [buf[0], buf[1]];
    let Ok(body_len) = u32::try_from(len - 2) else {
        return ValidationOutcome::Invalid;
    };
    for &(code, lo, hi) in VAT_COUNTRY_LENGTHS {
        if code == prefix {
            return ValidationOutcome::from_bool((lo..=hi).contains(&body_len));
        }
    }
    ValidationOutcome::Unknown
}
