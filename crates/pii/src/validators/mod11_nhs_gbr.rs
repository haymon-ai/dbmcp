//! UK NHS-number mod-11 checksum validator.

use super::prelude::*;

/// UK NHS number mod-11 validator.
///
/// Strips spaces / dashes; expects exactly 10 digits. Weights `[10..=2]` over
/// the first 9 digits; check digit = `(11 - sum%11) % 11`. A computed check
/// of `10` invalidates per the NHS specification (occurs when `sum % 11 == 1`).
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<10>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let sum: u32 = digits[..9]
        .iter()
        .zip([10u32, 9, 8, 7, 6, 5, 4, 3, 2])
        .map(|(d, w)| d * w)
        .sum();
    // NHS spec: check = 11 - (sum % 11). If that equals 11, store as 0; if 10, invalid.
    let check = match sum % 11 {
        0 => 0,
        1 => return ValidationOutcome::Invalid,
        n => 11 - n,
    };
    ValidationOutcome::from_bool(check == digits[9])
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn remainder_ten_branch_is_valid() {
        // sum%11 == 10 → check = 1. Pre-fix code wrongly rejected this branch.
        assert_eq!(validate("0000000051"), ValidationOutcome::Valid);
    }

    #[test]
    fn computed_check_ten_is_invalid() {
        // sum%11 == 1 → computed check = 10 → NHS spec says invalid.
        assert_eq!(validate("0000003000"), ValidationOutcome::Invalid);
    }
}
