//! Operators that rewrite a single matched span.

use std::borrow::Cow;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::Entity;

use super::{hash, mask};

/// Label the column rules' hash subkey is derived over.
const RULE_KEY_LABEL: &[u8] = b"dbmcp pii.rules hash key v1";

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
/// The derived `Debug` never shows the `Hash` key: `Hmac` prints `Hmac { ... }`.
#[derive(Debug, Clone)]
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

    /// Construct the column rules' hash operator, keyed by a subkey of `key`.
    ///
    /// The subkey is HMAC-SHA-256 under `key` over [`RULE_KEY_LABEL`], so
    /// rule tokens never equal the tokens [`Self::hash`] gives for `key`.
    /// Only the keyed HMAC state is kept, never `key` or the subkey.
    ///
    /// # Panics
    ///
    /// Never in practice: HMAC accepts keys of any length.
    #[must_use]
    pub(crate) fn rule_hash(key: &[u8]) -> Self {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(RULE_KEY_LABEL);
        Self::hash(&mac.finalize().into_bytes())
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
    use crate::Entity;

    #[test]
    fn rule_hash_differs_from_hash_under_the_same_key() {
        let key = b"0123456789abcdef0123456789abcdef";
        let span = "jane.doe@example.com";
        let global = Operator::hash(key).apply(Entity::EmailAddress, span).into_owned();
        let rule = Operator::rule_hash(key).apply(Entity::EmailAddress, span).into_owned();
        assert_eq!(global, "<EMAIL_ADDRESS_4e1f0dd69c51e84c>");
        assert_ne!(rule, global);
        assert_eq!(rule, Operator::rule_hash(key).apply(Entity::EmailAddress, span));
    }

    #[test]
    fn hash_debug_hides_the_key() {
        let key = "0123456789abcdef0123456789abcdef";
        let debug = format!("{:?}", Operator::hash(key.as_bytes()));
        assert_eq!(debug, "Hash { mac: Hmac { ... } }");
        assert!(!debug.contains(key));
    }
}
