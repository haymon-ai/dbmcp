//! US DEA Certificate Number checksum (Luhn-variant over 7 trailing digits).
//!
//! The DEA number is `<letter><letter|9><7 digits>`. The last digit is a
//! check digit derived from the first six per Drug Enforcement Administration
//! spec: `(2·(d1 + d3 + d5) + (d0 + d2 + d4)) mod 10 == check`. Letters and
//! the optional middle `9` are ignored by the math.

use super::digits::collect_digits;
use crate::ValidationOutcome;

pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = candidate.get(2..).and_then(collect_digits::<7>) else {
        return ValidationOutcome::Invalid;
    };
    let check = digits[6];
    let computed = (2 * (digits[1] + digits[3] + digits[5]) + (digits[0] + digits[2] + digits[4])) % 10;
    ValidationOutcome::from_bool(computed == check)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn valid_dea_passes() {
        assert_eq!(validate("AB1234563"), ValidationOutcome::Valid);
    }

    #[test]
    fn valid_dea_nine_prefix() {
        assert_eq!(validate("A91234563"), ValidationOutcome::Valid);
    }

    #[test]
    fn bad_checksum_rejected() {
        assert_eq!(validate("AB1234560"), ValidationOutcome::Invalid);
    }

    #[test]
    fn too_short_rejected() {
        assert_eq!(validate("AB"), ValidationOutcome::Invalid);
        assert_eq!(validate("AB12345"), ValidationOutcome::Invalid);
    }
}
