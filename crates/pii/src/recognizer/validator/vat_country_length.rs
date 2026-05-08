//! EU / UK VAT-number country-length validator.

use crate::recognizer::ValidationOutcome;

const VAT_COUNTRY_LENGTHS: &[([u8; 2], usize, usize)] = &[
    (*b"AT", 9, 9),
    (*b"BE", 10, 10),
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
    (*b"GB", 9, 12),
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

/// EU / UK VAT validator: known prefix, in-window body, at least one digit.
///
/// Format `<ISO2><alphanumeric>`. Returns [`ValidationOutcome::Valid`] only
/// when the prefix is in the EU / UK / XI table, the body length matches
/// the per-country window, and the body contains at least one ASCII digit.
/// All other cases return [`ValidationOutcome::Invalid`]. Unknown prefixes
/// and digit-less bodies are rejected to avoid all-uppercase English words
/// (e.g. `CERTIFICATE`, `INFRASTRUCTURE`) being tagged as VAT identifiers.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    // Candidate is the regex match: 9–14 ASCII uppercase alphanumeric bytes.
    let Some((prefix, body)) = candidate.as_bytes().split_first_chunk::<2>() else {
        return ValidationOutcome::Invalid;
    };
    let Some(&(_, lo, hi)) = VAT_COUNTRY_LENGTHS.iter().find(|(code, ..)| code == prefix) else {
        return ValidationOutcome::Invalid;
    };
    let has_digit = body.iter().any(u8::is_ascii_digit);
    ValidationOutcome::from_bool(has_digit && (lo..=hi).contains(&body.len()))
}
