//! Validator outcome enum.

/// Outcome of running a [`crate::validators::Validator`] on a candidate match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ValidationOutcome {
    /// Validator confirmed the candidate; promote to `MAX_SCORE`.
    Valid,
    /// Validator rejected the candidate; drop the result.
    Invalid,
    /// Validator abstained; leave the score untouched.
    Unknown,
}

impl ValidationOutcome {
    /// Map a boolean check to [`Self::Valid`] / [`Self::Invalid`].
    ///
    /// Use this when a validator's only outcomes are accept/reject — never
    /// abstain. Reduces the `if cond { Valid } else { Invalid }` boilerplate.
    #[must_use]
    pub const fn from_bool(valid: bool) -> Self {
        if valid { Self::Valid } else { Self::Invalid }
    }
}
