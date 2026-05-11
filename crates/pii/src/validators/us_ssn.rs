//! US SSN validator filtering reserved area, group, and serial values.

use super::digits::collect_digits;
use crate::ValidationOutcome;

/// US Social Security Number validator. Rejects reserved area / group / serial values.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let area = digits[0] * 100 + digits[1] * 10 + digits[2];
    let group = digits[3] * 10 + digits[4];
    let serial = digits[5] * 1000 + digits[6] * 100 + digits[7] * 10 + digits[8];
    let valid = area != 0 && area != 666 && area < 900 && group != 0 && serial != 0;
    ValidationOutcome::from_bool(valid)
}
