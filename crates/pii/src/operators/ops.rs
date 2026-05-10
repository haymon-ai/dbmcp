//! Operators that rewrite a single matched span and the operator-config map.

use std::borrow::Cow;

use crate::Entity;

use super::{hash, mask};

/// Hash algorithm for [`Operator::Hash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HashAlgorithm {
    /// SHA-256, 256-bit digest.
    Sha256,
    /// SHA-512, 512-bit digest.
    Sha512,
}

/// Mask coverage parameter for [`Operator::Mask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChunkCount {
    /// Mask the entire span, length-preserving.
    All,
    /// Mask exactly `n` UTF-8 code points.
    N(usize),
}

/// Algorithm used to rewrite a single PII span.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Operator {
    /// Replace the span with a fixed literal.
    Replace {
        /// Literal text written into the span.
        new_value: Cow<'static, str>,
    },
    /// Mask code points with `masking_char`.
    Mask {
        /// Character emitted in place of each masked code point.
        masking_char: char,
        /// How many code points to mask.
        chars_to_mask: ChunkCount,
        /// `true` keeps the span's prefix unmasked, `false` keeps the suffix.
        from_end: bool,
    },
    /// Replace the span with the empty string.
    Redact,
    /// Replace the span with a bare hex digest.
    Hash {
        /// Hash algorithm to use.
        algorithm: HashAlgorithm,
    },
}

/// Tag-only kind enum for audit-trail use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OperatorKind {
    /// [`Operator::Replace`].
    Replace,
    /// [`Operator::Mask`].
    Mask,
    /// [`Operator::Redact`].
    Redact,
    /// [`Operator::Hash`].
    Hash,
}

impl Operator {
    /// Default placeholder operator: `Replace { new_value: "<{entity_type}>" }`.
    ///
    /// Always borrows a `'static` placeholder from [`Entity::placeholder`] —
    /// zero allocation since the entity set is closed.
    #[must_use]
    pub const fn default_for(entity: Entity) -> Self {
        Self::Replace {
            new_value: Cow::Borrowed(entity.placeholder()),
        }
    }

    /// Default `Mask` per spec clarification: `'*'`, full span, length-preserving.
    #[must_use]
    pub fn default_mask() -> Self {
        Self::Mask {
            masking_char: '*',
            chars_to_mask: ChunkCount::All,
            from_end: true,
        }
    }

    /// Construct a hash operator.
    #[must_use]
    pub const fn hash(algorithm: HashAlgorithm) -> Self {
        Self::Hash { algorithm }
    }

    /// Tag describing this operator's variant.
    #[must_use]
    pub const fn kind(&self) -> OperatorKind {
        match self {
            Self::Replace { .. } => OperatorKind::Replace,
            Self::Mask { .. } => OperatorKind::Mask,
            Self::Redact => OperatorKind::Redact,
            Self::Hash { .. } => OperatorKind::Hash,
        }
    }

    /// Apply the operator to one matched span.
    ///
    /// Returns `Cow::Borrowed` for `Replace` and `Redact` (zero allocation);
    /// `Cow::Owned` for `Mask` and `Hash` (each writes a fresh String).
    pub(crate) fn apply<'a>(&'a self, candidate: &str) -> Cow<'a, str> {
        match self {
            Self::Replace { new_value } => Cow::Borrowed(new_value.as_ref()),
            Self::Mask {
                masking_char,
                chars_to_mask,
                from_end,
            } => Cow::Owned(mask::apply(candidate, *masking_char, *chars_to_mask, *from_end)),
            Self::Redact => Cow::Borrowed(""),
            Self::Hash { algorithm } => Cow::Owned(hash::apply(candidate, *algorithm)),
        }
    }
}
