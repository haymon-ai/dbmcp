//! Anonymizer engine: collapse overlaps, rewrite forward in a single pass.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::Entity;
use crate::operators::Operator;
use crate::overlap;
use crate::result::{OperatorResult, RecognizerResult};

/// Per-entity-type operator map handed to [`anonymize`].
#[derive(Debug, Clone, Default)]
pub struct OperatorConfig {
    /// Explicit overrides looked up by entity type.
    pub per_entity: HashMap<Entity, Operator>,
    /// Optional fallback when an entity type has no per-entity override.
    /// `None` means "use the entity-aware [`Operator::default_for`] placeholder".
    pub default: Option<Operator>,
}

impl OperatorConfig {
    /// Pick the operator for `entity`. Borrows from the config when possible;
    /// only allocates a fresh placeholder when neither a per-entity override nor
    /// `default` is set.
    fn select(&self, entity: Entity) -> Cow<'_, Operator> {
        if let Some(op) = self.per_entity.get(&entity) {
            return Cow::Borrowed(op);
        }
        if let Some(default) = &self.default {
            return Cow::Borrowed(default);
        }
        Cow::Owned(Operator::default_for(entity))
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

        let operator = config.select(entity_type);
        let rewritten = operator.apply(entity_type, &text[start..end]);
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
    use crate::operators::OperatorKind;
    use crate::result::AnalysisExplanation;
    use crate::score::Score;
    use crate::validation::ValidationOutcome;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
    const TEXT: &str = "jane.doe@example.com";

    fn email_span() -> Vec<RecognizerResult> {
        let score = Score::from_static(0.9);
        vec![RecognizerResult {
            entity_type: Entity::EmailAddress,
            start: 0,
            end: TEXT.len(),
            score,
            explanation: AnalysisExplanation {
                recognizer_name: Cow::Borrowed("test"),
                pattern_name: None,
                original_score: score,
                validation: ValidationOutcome::Unknown,
                final_score: score,
                supportive_keyword: None,
            },
        }]
    }

    fn with_default(operator: Operator) -> OperatorConfig {
        OperatorConfig {
            default: Some(operator),
            ..OperatorConfig::default()
        }
    }

    #[test]
    fn hash_default_emits_keyed_token_and_hash_kind() {
        let out = anonymize(TEXT, email_span(), &with_default(Operator::hash(KEY)));
        assert_eq!(out.text, "<EMAIL_ADDRESS_4e1f0dd69c51e84c>");
        assert_eq!(out.operations[0].operator, OperatorKind::Hash);
    }

    #[test]
    fn per_entity_override_beats_hash_default() {
        let mut config = with_default(Operator::hash(KEY));
        config.per_entity.insert(Entity::EmailAddress, Operator::Redact);
        assert_eq!(anonymize(TEXT, email_span(), &config).text, "");
    }

    #[test]
    fn key_free_operators_are_unchanged() {
        assert_eq!(
            anonymize(TEXT, email_span(), &OperatorConfig::default()).text,
            "<EMAIL_ADDRESS>"
        );
        assert_eq!(
            anonymize(TEXT, email_span(), &with_default(Operator::default_mask())).text,
            "*".repeat(TEXT.len())
        );
        assert_eq!(anonymize(TEXT, email_span(), &with_default(Operator::Redact)).text, "");
    }
}
