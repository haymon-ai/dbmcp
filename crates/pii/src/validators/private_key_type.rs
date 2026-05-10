//! PEM private-key block type validator.

use crate::ValidationOutcome;

/// PEM private-key block type validator: BEGIN-type MUST equal END-type.
///
/// The recognizer regex already enforces that the BEGIN/END labels end in
/// `PRIVATE KEY`, so this validator only checks the labels match each other.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(begin_label) = candidate
        .split_once("-----BEGIN ")
        .and_then(|(_, rest)| rest.split_once("-----"))
        .map(|(label, _)| label.trim())
    else {
        return ValidationOutcome::Invalid;
    };
    let Some(end_label) = candidate
        .rsplit_once("-----END ")
        .and_then(|(_, rest)| rest.split_once("-----"))
        .map(|(label, _)| label.trim())
    else {
        return ValidationOutcome::Invalid;
    };
    ValidationOutcome::from_bool(begin_label == end_label)
}
