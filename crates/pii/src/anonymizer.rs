//! Anonymizer engine: collapse overlaps, rewrite forward in a single pass.

use std::borrow::Cow;
use std::collections::HashMap;

use dbmcp_config::PiiOperator;

use crate::operator::Operator;
use crate::overlap;
use crate::recognizer::EntityType;
use crate::result::{OperatorResult, RecognizerResult};

/// Per-entity-type operator map handed to [`anonymize`].
#[derive(Debug, Clone, Default)]
pub struct OperatorConfig {
    /// Explicit overrides looked up by entity type.
    pub per_entity: HashMap<EntityType, Operator>,
    /// Optional fallback when an entity type has no per-entity override.
    /// `None` means "use the entity-aware [`Operator::default_for`] placeholder".
    pub default: Option<Operator>,
}

impl OperatorConfig {
    /// Pick the operator for `entity_type`. Borrows from the config when possible;
    /// only allocates a fresh placeholder when neither a per-entity override nor
    /// `default` is set.
    fn select(&self, entity_type: &EntityType) -> Cow<'_, Operator> {
        if let Some(op) = self.per_entity.get(entity_type) {
            return Cow::Borrowed(op);
        }
        if let Some(default) = &self.default {
            return Cow::Borrowed(default);
        }
        Cow::Owned(Operator::default_for(entity_type))
    }
}

impl From<PiiOperator> for OperatorConfig {
    fn from(op: PiiOperator) -> Self {
        use crate::operator::HashAlgorithm;
        let default = match op {
            PiiOperator::Replace => None,
            PiiOperator::Mask => Some(Operator::default_mask()),
            PiiOperator::Redact => Some(Operator::Redact),
            PiiOperator::Hash => Some(Operator::hash(HashAlgorithm::Sha256, None).expect("None hash_key never errors")),
        };
        Self {
            per_entity: HashMap::new(),
            default,
        }
    }
}

/// Output of [`anonymize`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnonymizedText {
    /// Rewritten text.
    pub text: String,
    /// Operator audit trail in original-position order.
    pub operations: Vec<OperatorResult>,
}

/// Apply per-entity operators in a single forward pass.
///
/// Steps: (1) collapse overlaps via [`overlap::resolve`] — which returns survivors in
/// start-ascending order; (2) walk left-to-right, appending the gap between cursor and
/// span verbatim, then the operator output. Time complexity is
/// `O(text_len + Σ rewrites)`.
#[must_use]
pub fn anonymize(text: &str, results: Vec<RecognizerResult>, config: &OperatorConfig) -> AnonymizedText {
    let surviving = overlap::resolve(results);
    if surviving.is_empty() {
        return AnonymizedText {
            text: text.to_owned(),
            operations: Vec::new(),
        };
    }

    let mut new_text = String::with_capacity(text.len());
    let mut operations = Vec::with_capacity(surviving.len());
    let mut cursor = 0usize;

    for result in surviving {
        let RecognizerResult {
            entity_type,
            start,
            end,
            ..
        } = result;
        if start < cursor
            || end < start
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        new_text.push_str(&text[cursor..start]);
        let new_start = new_text.len();

        let operator = config.select(&entity_type);
        let rewritten = operator.apply(&text[start..end]);
        new_text.push_str(&rewritten);
        let new_end = new_text.len();

        operations.push(OperatorResult {
            entity_type,
            operator: operator.kind(),
            original_start: start,
            original_end: end,
            new_start,
            new_end,
        });
        cursor = end;
    }
    new_text.push_str(&text[cursor..]);

    AnonymizedText {
        text: new_text,
        operations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator::HashAlgorithm;

    #[test]
    fn pii_operator_replace_maps_to_default_none() {
        let cfg: OperatorConfig = PiiOperator::Replace.into();
        assert!(cfg.default.is_none(), "Replace must defer to entity-aware placeholder");
        assert!(cfg.per_entity.is_empty());
    }

    #[test]
    fn pii_operator_mask_maps_to_default_mask() {
        let cfg: OperatorConfig = PiiOperator::Mask.into();
        assert!(matches!(cfg.default, Some(Operator::Mask { .. })));
    }

    #[test]
    fn pii_operator_redact_maps_to_redact_variant() {
        let cfg: OperatorConfig = PiiOperator::Redact.into();
        assert!(matches!(cfg.default, Some(Operator::Redact)));
    }

    #[test]
    fn pii_operator_hash_maps_to_sha256_no_key() {
        let cfg: OperatorConfig = PiiOperator::Hash.into();
        assert!(matches!(
            cfg.default,
            Some(Operator::Hash {
                algorithm: HashAlgorithm::Sha256,
                hash_key: None,
            })
        ));
    }
}
