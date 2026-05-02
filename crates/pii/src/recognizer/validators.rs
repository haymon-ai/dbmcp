//! Built-in validators: `Luhn`, `IBAN` mod-97, and IP-address parse-validation.

use std::net::IpAddr;
use std::str::FromStr;

use super::{ValidationOutcome, Validator};

/// Default validator that abstains on every input.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopValidator;

impl Validator for NoopValidator {
    fn validate(&self, _candidate: &str) -> ValidationOutcome {
        ValidationOutcome::Unknown
    }
}

/// Luhn checksum validator for credit-card numbers.
///
/// Strips spaces and dashes before checking, matching Presidio's
/// `replacement_pairs = [("-", ""), (" ", "")]`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LuhnValidator;

impl Validator for LuhnValidator {
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        // Buffer fits the longest valid card (19 digits); avoids a heap allocation.
        let mut digits = [0u8; 19];
        let mut len = 0usize;
        for d in candidate.chars().filter_map(|c| c.to_digit(10)) {
            if len == digits.len() {
                return ValidationOutcome::Invalid;
            }
            digits[len] = u8::try_from(d).expect("base-10 digit fits in u8");
            len += 1;
        }
        if !(12..=19).contains(&len) {
            return ValidationOutcome::Invalid;
        }
        let mut sum: u32 = 0;
        for (i, &d) in digits[..len].iter().rev().enumerate() {
            let mut n = u32::from(d);
            if !i.is_multiple_of(2) {
                n *= 2;
                if n > 9 {
                    n -= 9;
                }
            }
            sum += n;
        }
        if sum.is_multiple_of(10) {
            ValidationOutcome::Valid
        } else {
            ValidationOutcome::Invalid
        }
    }
}

/// IBAN mod-97 validator. Accepts upper-case input; whitespace stripped before checking.
#[derive(Debug, Default, Clone, Copy)]
pub struct IbanValidator;

impl Validator for IbanValidator {
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        // Longest legal IBAN is 34 chars; stack-buffer the cleaned form.
        let mut buf = [0u8; 34];
        let mut len = 0usize;
        for c in candidate.chars().filter(|c| !c.is_whitespace()) {
            let upper = c.to_ascii_uppercase();
            if len == buf.len() || !upper.is_ascii() {
                return ValidationOutcome::Invalid;
            }
            buf[len] = upper as u8;
            len += 1;
        }
        if len < 15 {
            return ValidationOutcome::Invalid;
        }
        // Rearranged = tail (positions 4..len) followed by head (positions 0..4).
        let rearranged = buf[4..len].iter().chain(buf[..4].iter()).copied();
        match mod97(rearranged) {
            Some(1) => ValidationOutcome::Valid,
            _ => ValidationOutcome::Invalid,
        }
    }
}

fn mod97<I: Iterator<Item = u8>>(bytes: I) -> Option<u32> {
    let mut remainder: u32 = 0;
    for b in bytes {
        if b.is_ascii_digit() {
            remainder = (remainder * 10 + u32::from(b - b'0')) % 97;
        } else if b.is_ascii_uppercase() {
            remainder = (remainder * 100 + u32::from(b - b'A' + 10)) % 97;
        } else {
            return None;
        }
    }
    Some(remainder)
}

/// US Social Security Number validator. Rejects reserved area / group / serial values
/// — replaces the negative-lookahead constructs Presidio's regex used.
#[derive(Debug, Default, Clone, Copy)]
pub struct UsSsnValidator;

impl Validator for UsSsnValidator {
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        let mut digits = [0u32; 9];
        let mut len = 0usize;
        for d in candidate.chars().filter_map(|c| c.to_digit(10)) {
            if len == digits.len() {
                return ValidationOutcome::Invalid;
            }
            digits[len] = d;
            len += 1;
        }
        if len != 9 {
            return ValidationOutcome::Invalid;
        }
        let area = digits[0] * 100 + digits[1] * 10 + digits[2];
        let group = digits[3] * 10 + digits[4];
        let serial = digits[5] * 1000 + digits[6] * 100 + digits[7] * 10 + digits[8];
        if area == 0 || area == 666 || area >= 900 || group == 0 || serial == 0 {
            return ValidationOutcome::Invalid;
        }
        ValidationOutcome::Valid
    }
}

/// IP-address validator that delegates to [`std::net::IpAddr::from_str`].
///
/// CIDR-like suffixes (`/24`, `/64`) are stripped before parsing; only the
/// address portion is parse-validated. A bare IPv6 zone identifier (`%eth0`)
/// is also stripped because `from_str` rejects it on stable today.
#[derive(Debug, Default, Clone, Copy)]
pub struct IpAddressValidator;

impl Validator for IpAddressValidator {
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        // Strip CIDR suffix `/N` and IPv6 zone identifier `%zone` in one split.
        let trimmed = candidate.split(['/', '%']).next().unwrap_or("");
        if IpAddr::from_str(trimmed).is_ok() {
            ValidationOutcome::Valid
        } else {
            ValidationOutcome::Invalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IbanValidator, IpAddressValidator, LuhnValidator, ValidationOutcome, Validator};

    #[test]
    fn luhn_valid_visa() {
        assert_eq!(LuhnValidator.validate("4111-1111-1111-1111"), ValidationOutcome::Valid);
    }

    #[test]
    fn luhn_invalid_visa() {
        assert_eq!(
            LuhnValidator.validate("4111-1111-1111-1112"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn luhn_rejects_short() {
        assert_eq!(LuhnValidator.validate("4111111"), ValidationOutcome::Invalid);
    }

    #[test]
    fn iban_valid_de() {
        // Wikipedia example
        assert_eq!(
            IbanValidator.validate("DE89 3704 0044 0532 0130 00"),
            ValidationOutcome::Valid
        );
    }

    #[test]
    fn iban_invalid_check_digits() {
        assert_eq!(
            IbanValidator.validate("DE00 3704 0044 0532 0130 00"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn ip_valid_v4() {
        assert_eq!(IpAddressValidator.validate("192.168.1.1"), ValidationOutcome::Valid);
    }

    #[test]
    fn ip_invalid_v4() {
        assert_eq!(IpAddressValidator.validate("192.168.1.999"), ValidationOutcome::Invalid);
    }

    #[test]
    fn ip_valid_v6() {
        assert_eq!(IpAddressValidator.validate("::1"), ValidationOutcome::Valid);
    }

    #[test]
    fn ip_with_cidr_suffix() {
        assert_eq!(IpAddressValidator.validate("10.0.0.0/24"), ValidationOutcome::Valid);
    }
}
