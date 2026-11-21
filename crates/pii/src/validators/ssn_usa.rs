//! US SSN validator filtering reserved values and well-known placeholders.

use super::prelude::*;

/// Widely published example SSNs.
const PLACEHOLDERS: [[u32; 9]; 2] = [[1, 2, 3, 4, 5, 6, 7, 8, 9], [0, 7, 8, 0, 5, 1, 1, 2, 0]];

/// US Social Security Number validator.
///
/// Rejects reserved area / group / serial values, one digit repeated, and
/// [`PLACEHOLDERS`].
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let area = digits[0] * 100 + digits[1] * 10 + digits[2];
    let group = digits[3] * 10 + digits[4];
    let serial = digits[5] * 1000 + digits[6] * 100 + digits[7] * 10 + digits[8];
    let repeated = digits.iter().all(|&d| d == digits[0]);
    let valid = area != 0
        && area != 666
        && area < 900
        && group != 0
        && serial != 0
        && !repeated
        && !PLACEHOLDERS.contains(&digits);
    ValidationOutcome::from_bool(valid)
}
