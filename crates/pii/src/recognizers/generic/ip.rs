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

#[cfg(test)]
mod tests {
    use super::ip_address;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        ip_address()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_ip_address() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("haymon.ai 192.168.0.1", &[(10, 21)]),
            ("10.0.0.0/24", &[(0, 11)]),
            ("my ip: 192.168.0", &[]),
            ("192.168.1.999", &[]),
            ("256.256.256.256", &[]),
            ("haymon.ai 684D:1111:222:3333:4444:5555:6:77", &[(10, 43)]),
            ("my ip: 684D:1111:222:3333:4444:5555:6:77", &[(7, 40)]),
            ("684D:1111:222:3333:4444:5555:77", &[]),
            ("my ip: ::1", &[(7, 10)]),
            ("connecting from ::1", &[(16, 19)]),
            ("2400:c401::5054:ff:fe1b:b031", &[(0, 28)]),
            ("fe80::1", &[(0, 7)]),
            ("2001:db8::8a2e:370:7334", &[(0, 23)]),
            ("2001:db8::1", &[(0, 11)]),
            ("Server IP: 2001:db8::1", &[(11, 22)]),
            ("Connect to [2001:db8::1]:8080", &[(12, 23)]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
