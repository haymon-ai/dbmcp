//! US ABA routing-number checksum validator.

use super::digits::collect_digits;
use crate::ValidationOutcome;

/// US ABA routing-number checksum.
///
/// Nine digits required; valid iff `(3·d1 + 7·d2 + d3 + 3·d4 + 7·d5 + d6 + 3·d7 + 7·d8 + d9) % 10 == 0`.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(d) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    let weights = [3u32, 7, 1, 3, 7, 1, 3, 7, 1];
    let sum: u32 = d.iter().zip(weights).map(|(x, w)| x * w).sum();
    ValidationOutcome::from_bool(sum.is_multiple_of(10))
}
