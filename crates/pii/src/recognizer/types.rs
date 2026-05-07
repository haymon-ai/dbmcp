//! Entity-type newtype and validator outcome enum.

use std::borrow::Cow;

/// Tag identifying the kind of PII a recognizer emits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EntityType(pub(crate) Cow<'static, str>);

impl EntityType {
    /// Build an entity type from any string-like source.
    #[must_use]
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    /// Return the entity-type name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Outcome of running a [`super::Validator`] on a candidate match.
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
