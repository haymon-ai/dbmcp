//! `IP_ADDRESS` recognizer (IPv4 + IPv6) with parse-validation.
//!
//! Shape filtering happens at the regex layer; [`IpAddressValidator`] delegates
//! the precise validity check to [`std::net::IpAddr::from_str`]. False
//! positives the regex lets through are dropped by the parser.

use crate::recognizers::prelude::*;

/// Context keywords for IP addresses.
const CONTEXT: &[&str] = &["ip", "ipv4", "ipv6"];

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
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, ip_address};

    #[test]
    fn carries_context_list() {
        assert_eq!(ip_address().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        ip_address()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_ip_address() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("haymon.ai 192.168.0.1", &[("192.168.0.1", 1.0)]),
            ("10.0.0.0/24", &[("10.0.0.0/24", 1.0)]),
            ("my ip: 192.168.0", &[]),
            ("192.168.1.999", &[]),
            ("256.256.256.256", &[]),
            (
                "haymon.ai 684D:1111:222:3333:4444:5555:6:77",
                &[("684D:1111:222:3333:4444:5555:6:77", 1.0)],
            ),
            (
                "my ip: 684D:1111:222:3333:4444:5555:6:77",
                &[("684D:1111:222:3333:4444:5555:6:77", 1.0)],
            ),
            ("684D:1111:222:3333:4444:5555:77", &[]),
            ("my ip: ::1", &[("::1", 1.0)]),
            ("connecting from ::1", &[("::1", 1.0)]),
            ("2400:c401::5054:ff:fe1b:b031", &[("2400:c401::5054:ff:fe1b:b031", 1.0)]),
            ("fe80::1", &[("fe80::1", 1.0)]),
            ("2001:db8::8a2e:370:7334", &[("2001:db8::8a2e:370:7334", 1.0)]),
            ("2001:db8::1", &[("2001:db8::1", 1.0)]),
            ("Server IP: 2001:db8::1", &[("2001:db8::1", 1.0)]),
            ("Connect to [2001:db8::1]:8080", &[("2001:db8::1", 1.0)]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
