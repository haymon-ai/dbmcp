//! `IP_ADDRESS` recognizer (IPv4 + IPv6) with parse-validation.
//!
//! Shape filtering happens at the regex layer; [`IpAddressValidator`] delegates
//! the precise validity check to [`std::net::IpAddr::from_str`]. False
//! positives the regex lets through are dropped by the parser.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `IP_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn ip_address() -> Recognizer {
    let s06 = Score::from_static(0.6);

    let ipv4 =
        Pattern::new("IPv4", r"\b\d{1,3}(?:\.\d{1,3}){3}(?:/\d{1,2})?\b", s06).expect("static IPv4 pattern compiles");

    let ipv6 = Pattern::new(
        "IPv6",
        r"\b(?:[0-9A-Fa-f]{1,4}:){7}[0-9A-Fa-f]{1,4}(?:/\d{1,3})?\b|\b(?:[0-9A-Fa-f]{1,4}:){1,6}:[0-9A-Fa-f]{1,4}(?::[0-9A-Fa-f]{1,4})*(?:/\d{1,3})?\b|::[0-9A-Fa-f]{1,4}(?::[0-9A-Fa-f]{1,4})*(?:/\d{1,3})?\b|\b(?:[0-9A-Fa-f]{1,4}:){2,7}:(?:/\d{1,3})?\b",
        s06,
    )
    .expect("static IPv6 pattern compiles");

    Recognizer::new(Entity::IpAddress, vec![ipv4, ipv6])
        .expect("non-empty pattern list")
        .with_name("IpRecognizer")
        .with_validator(Validator::IpAddress)
        .with_category(Category::Network)
}
