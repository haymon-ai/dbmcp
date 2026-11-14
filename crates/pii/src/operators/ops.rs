//! Operators that rewrite a single matched span.

use std::borrow::Cow;
use std::fmt;

use hmac::{Hmac, KeyInit};
use sha2::Sha256;

use crate::Entity;

use super::{hash, mask};

/// Mask coverage parameter for [`Operator::Mask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChunkCount {
    /// Mask the entire span, length-preserving.
    All,
    /// Mask exactly `n` UTF-8 code points.
    N(usize),
}

/// Algorithm used to rewrite a single PII span.
///
/// `Debug` is hand-written so the `Hash` key never reaches a log.
#[derive(Clone)]
pub enum Operator {
    /// Replace the span with its entity placeholder `<{TYPE}>`.
    Replace,
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
    /// Replace the span with a keyed token `<{TYPE}_{16 hex}>`.
    Hash {
        /// HMAC-SHA-256 state, already keyed; the raw key is not kept.
        mac: Hmac<Sha256>,
    },
}

impl fmt::Debug for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Replace => f.write_str("Replace"),
            Self::Mask {
                masking_char,
                chars_to_mask,
                from_end,
            } => f
                .debug_struct("Mask")
                .field("masking_char", masking_char)
                .field("chars_to_mask", chars_to_mask)
                .field("from_end", from_end)
                .finish(),
            Self::Redact => f.write_str("Redact"),
            Self::Hash { .. } => f.write_str("Hash { key: <redacted> }"),
        }
    }
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
    /// Default `Mask` per spec clarification: `'*'`, full span, length-preserving.
    #[must_use]
    pub fn default_mask() -> Self {
        Self::Mask {
            masking_char: '*',
            chars_to_mask: ChunkCount::All,
            from_end: true,
        }
    }

    /// Construct a keyed hash operator from the raw HMAC key bytes.
    ///
    /// Only the keyed HMAC state is kept, never `key` itself.
    ///
    /// # Panics
    ///
    /// Never in practice: HMAC accepts keys of any length.
    #[must_use]
    pub fn hash(key: &[u8]) -> Self {
        Self::Hash {
            mac: Hmac::new_from_slice(key).expect("HMAC accepts any key length"),
        }
    }

    /// Tag describing this operator's variant.
    #[must_use]
    pub const fn kind(&self) -> OperatorKind {
        match self {
            Self::Replace => OperatorKind::Replace,
            Self::Mask { .. } => OperatorKind::Mask,
            Self::Redact => OperatorKind::Redact,
            Self::Hash { .. } => OperatorKind::Hash,
        }
    }

    /// Apply the operator to one matched span of type `entity`.
    ///
    /// Returns `Cow::Borrowed` for `Replace` (the `'static` [`Entity::placeholder`])
    /// and `Redact` (zero allocation);
    /// `Cow::Owned` for `Mask` and `Hash` (each writes a fresh String).
    pub(crate) fn apply<'a>(&'a self, entity: Entity, candidate: &str) -> Cow<'a, str> {
        match self {
            Self::Replace => Cow::Borrowed(entity.placeholder()),
            Self::Mask {
                masking_char,
                chars_to_mask,
                from_end,
            } => Cow::Owned(mask::apply(candidate, *masking_char, *chars_to_mask, *from_end)),
            Self::Redact => Cow::Borrowed(""),
            Self::Hash { mac } => Cow::Owned(hash::apply(mac, entity, candidate)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Operator;

    #[test]
    fn hash_debug_hides_the_key() {
        let key = "0123456789abcdef0123456789abcdef";
        let debug = format!("{:?}", Operator::hash(key.as_bytes()));
        assert_eq!(debug, "Hash { key: <redacted> }");
        assert!(!debug.contains(key));
    }
}
