//! Luhn checksum validator gated to exactly 9 digits, used by `SIN_CA`.

use super::digits::collect_digits;
use super::luhn::luhn_passes;
use crate::recognizer::ValidationOutcome;

/// Luhn checksum gated to exactly 9 digits, used by `SIN_CA`.
///
/// The card-flavoured [`super::Validator::Luhn`] requires 12–19 digits and would
/// reject a 9-digit Canadian SIN. This sibling validator runs the same Luhn
/// algorithm over exactly 9 digits.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(digits) = collect_digits::<9>(candidate) else {
        return ValidationOutcome::Invalid;
    };
    ValidationOutcome::from_bool(luhn_passes(digits))
}
