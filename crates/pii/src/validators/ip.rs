//! IP-address validator delegating to [`std::net::IpAddr::from_str`].

use std::net::IpAddr;
use std::str::FromStr;

use super::prelude::*;

/// IP-address validator that delegates to [`std::net::IpAddr::from_str`].
///
/// CIDR-like suffixes (`/24`, `/64`) are stripped before parsing; only the
/// address portion is parse-validated. A bare IPv6 zone identifier (`%eth0`)
/// is also stripped because `from_str` rejects it on stable today.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    // Strip CIDR suffix `/N` and IPv6 zone identifier `%zone` in one split.
    let trimmed = candidate.split(['/', '%']).next().unwrap_or("");
    ValidationOutcome::from_bool(IpAddr::from_str(trimmed).is_ok())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn ip_valid_v4() {
        assert_eq!(validate("192.168.1.1"), ValidationOutcome::Valid);
    }

    #[test]
    fn ip_invalid_v4() {
        assert_eq!(validate("192.168.1.999"), ValidationOutcome::Invalid);
    }

    #[test]
    fn ip_valid_v6() {
        assert_eq!(validate("::1"), ValidationOutcome::Valid);
    }

    #[test]
    fn ip_with_cidr_suffix() {
        assert_eq!(validate("10.0.0.0/24"), ValidationOutcome::Valid);
    }
}
