//! US ITIN middle-block range validator.

use super::digits::collect_digits;
use crate::ValidationOutcome;

/// US ITIN middle-block range validator.
///
/// Format `9XX-NN-NNNN`. Middle digits MUST be in `70-88 ∪ 90-92 ∪ 94-99` per IRS rules.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(d) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    // Regex anchors first digit to `9`, so we only check the middle block here.
    let middle = d[3] * 10 + d[4];
    let valid = (70..=88).contains(&middle) || (90..=92).contains(&middle) || (94..=99).contains(&middle);
    ValidationOutcome::from_bool(valid)
}
