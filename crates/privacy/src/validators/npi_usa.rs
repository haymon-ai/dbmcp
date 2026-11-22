//! US NPI checksum (CMS Luhn-with-`80840`-prefix) plus degenerate-body filter.
//!
//! Per the CMS NPI Standard, a 10-digit National Provider Identifier is
//! validated by prepending the constant `"80840"` and running the standard
//! Luhn algorithm over the resulting 15 digits. The validator additionally
//! rejects "degenerate" numbers whose 9-digit body is a single repeated digit
//! (e.g. `1111111110`); without this filter the Luhn check passes for several
//! such sequences and produces noisy false positives.

use super::luhn::luhn_passes;
use super::prelude::*;

const NPI_PREFIX: [u32; 5] = [8, 0, 8, 4, 0];

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<10>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let body = &digits[..9];
    if body.iter().all(|d| *d == body[0]) {
        return ValidationOutcome::Invalid;
    }
    let full = NPI_PREFIX.iter().chain(digits.iter()).copied();
    ValidationOutcome::from_bool(luhn_passes(full))
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn valid_npi_passes() {
        assert_eq!(validate("1234567893"), ValidationOutcome::Valid);
    }

    #[test]
    fn dashed_npi_passes() {
        assert_eq!(validate("1234-567-893"), ValidationOutcome::Valid);
    }

    #[test]
    fn spaced_npi_passes() {
        assert_eq!(validate("1234 567 893"), ValidationOutcome::Valid);
    }

    #[test]
    fn bad_checksum_rejected() {
        assert_eq!(validate("1234567890"), ValidationOutcome::Invalid);
    }

    #[test]
    fn wrong_length_rejected() {
        assert_eq!(validate("12345"), ValidationOutcome::Invalid);
        assert_eq!(validate("12345678901"), ValidationOutcome::Invalid);
    }

    #[test]
    fn degenerate_body_rejected() {
        assert_eq!(validate("9999999995"), ValidationOutcome::Invalid);
    }
}
