//! `IP_ADDRESS` recognizer (IPv4 + IPv6) with parse-validation.
//!
//! Patterns are deliberately permissive shape filters; [`IpAddressValidator`]
//! delegates the precise validity check to [`std::net::IpAddr::from_str`]
//! (best practice — `IpAddr` already encodes every IPv6 compression /
//! mixed-notation rule). False positives the regex lets through are dropped
//! by the parser.

use crate::recognizer::{Category, IpAddressValidator, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `IP_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn ip_address() -> Pattern {
    let s06 = Score::from_static(0.6);

    // Four dotted decimal triplets (1–3 digits each), optional CIDR /N.
    // The validator parses with `Ipv4Addr::from_str` so triple-digit overflows
    // (e.g. `999`) are caught there, not here.
    let ipv4 =
        Regex::new("IPv4", r"\b\d{1,3}(?:\.\d{1,3}){3}(?:/\d{1,2})?\b", s06).expect("static IPv4 pattern compiles");

    // 2–8 hex groups separated by `:`, with at most one `::` compression and an
    // optional CIDR suffix. The parser does the heavy lifting.
    let ipv6 = Regex::new("IPv6", r"(?:[0-9A-Fa-f]{0,4}:){1,7}[0-9A-Fa-f]{0,4}(?:/\d{1,3})?", s06)
        .expect("static IPv6 pattern compiles");

    Pattern::new(entity::IP_ADDRESS, vec![ipv4, ipv6])
        .expect("non-empty pattern list")
        .with_name("IpRecognizer")
        .with_validator(IpAddressValidator)
        .with_category(Category::Network)
}
