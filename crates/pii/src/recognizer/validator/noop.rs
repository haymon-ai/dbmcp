//! Default validator that abstains on every candidate.

use crate::recognizer::{ValidationOutcome, Validator};

/// Default validator that abstains on every input.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopValidator;

impl Validator for NoopValidator {
    fn validate(&self, _candidate: &str) -> ValidationOutcome {
        ValidationOutcome::Unknown
    }
}
