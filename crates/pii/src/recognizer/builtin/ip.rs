//! `IP_ADDRESS` recognizer (IPv4 + IPv6) with parse-validation.
//!
//! Uses plain `regex` patterns. Boundary anchoring uses `\b` for IPv4 and
//! tightened IPv6 patterns; rare false-positive matches inside non-IP tokens
//! are caught by [`IpAddressValidator`] (parse-validation drops them).

use crate::pattern::Pattern;
use crate::recognizer::{IpAddressValidator, PatternRecognizer, entity};
use crate::score::Score;

/// Build the `IP_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn ip_address() -> PatternRecognizer {
    let s06 = Score::from_static(0.6);

    let ipv4 = Pattern::new(
        "IPv4",
        r"\b(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:/(?:[0-2]?\d|3[0-2]))?\b",
        s06,
    )
    .expect("static IPv4 pattern compiles");

    let ipv6 = Pattern::new(
        "IPv6",
        r"(?:(?:[0-9A-Fa-f]{1,4}:){7}[0-9A-Fa-f]{1,4}|(?:[0-9A-Fa-f]{1,4}:){1,7}:|:(?::[0-9A-Fa-f]{1,4}){1,7}|(?:[0-9A-Fa-f]{1,4}:){1,6}:[0-9A-Fa-f]{1,4}|(?:[0-9A-Fa-f]{1,4}:){1,5}(?::[0-9A-Fa-f]{1,4}){1,2}|(?:[0-9A-Fa-f]{1,4}:){1,4}(?::[0-9A-Fa-f]{1,4}){1,3}|(?:[0-9A-Fa-f]{1,4}:){1,3}(?::[0-9A-Fa-f]{1,4}){1,4}|(?:[0-9A-Fa-f]{1,4}:){1,2}(?::[0-9A-Fa-f]{1,4}){1,5}|[0-9A-Fa-f]{1,4}:(?::[0-9A-Fa-f]{1,4}){1,6}|:(?::[0-9A-Fa-f]{1,4}){1,6})(?:/(?:12[0-8]|1[01]\d|[1-9]?\d))?",
        s06,
    )
    .expect("static IPv6 pattern compiles");

    PatternRecognizer::new(entity::IP_ADDRESS, vec![ipv4, ipv6])
        .expect("non-empty pattern list")
        .with_name("IpRecognizer")
        .with_validator(IpAddressValidator)
}
