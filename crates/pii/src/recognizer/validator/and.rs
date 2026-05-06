//! Combinator validator with AND semantics.

use crate::recognizer::{ValidationOutcome, Validator};

/// Combinator returning [`ValidationOutcome::Valid`] only if both children agree.
///
/// Truth table:
/// - Both `Valid` → `Valid`
/// - Either `Invalid` → `Invalid` (short-circuits)
/// - Otherwise → `Unknown`
#[derive(Debug, Clone, Copy)]
pub struct AndValidator<L, R> {
    /// Left-hand operand (evaluated first).
    pub left: L,
    /// Right-hand operand.
    pub right: R,
}

impl<L, R> AndValidator<L, R> {
    /// Compose two validators with AND semantics.
    pub const fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> Validator for AndValidator<L, R>
where
    L: Validator,
    R: Validator,
{
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        match (self.left.validate(candidate), self.right.validate(candidate)) {
            (ValidationOutcome::Invalid, _) | (_, ValidationOutcome::Invalid) => ValidationOutcome::Invalid,
            (ValidationOutcome::Valid, ValidationOutcome::Valid) => ValidationOutcome::Valid,
            _ => ValidationOutcome::Unknown,
        }
    }

    fn validate_with_context(
        &self,
        candidate: &str,
        full_text: &str,
        span: std::ops::Range<usize>,
    ) -> ValidationOutcome {
        let l = self.left.validate_with_context(candidate, full_text, span.clone());
        if matches!(l, ValidationOutcome::Invalid) {
            return ValidationOutcome::Invalid;
        }
        let r = self.right.validate_with_context(candidate, full_text, span);
        match (l, r) {
            (_, ValidationOutcome::Invalid) => ValidationOutcome::Invalid,
            (ValidationOutcome::Valid, ValidationOutcome::Valid) => ValidationOutcome::Valid,
            _ => ValidationOutcome::Unknown,
        }
    }
}
