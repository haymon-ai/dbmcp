//! Typed PII span returned by recognizers and the analyzer engine.

use std::borrow::Cow;

use crate::recognizer::{EntityType, ValidationOutcome};
use crate::score::Score;

/// One detected PII span with audit trail.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecognizerResult {
    /// Entity type emitted by the recognizer.
    pub entity_type: EntityType,
    /// Inclusive start byte offset into the original input.
    pub start: usize,
    /// Exclusive end byte offset into the original input.
    pub end: usize,
    /// Post-validator confidence score.
    pub score: Score,
    /// Audit trail describing why this span was flagged.
    pub explanation: AnalysisExplanation,
}

/// Audit metadata for a single [`RecognizerResult`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnalysisExplanation {
    /// Recognizer that emitted the result.
    pub recognizer_name: Cow<'static, str>,
    /// Pattern name that matched (`None` for non-pattern recognizers, future use).
    pub pattern_name: Option<Cow<'static, str>>,
    /// Pattern's base score, before validator adjustment.
    pub original_score: Score,
    /// Validator outcome applied to the candidate.
    pub validation: ValidationOutcome,
    /// Final score after validator adjustment; equals `RecognizerResult.score`.
    pub final_score: Score,
}

/// Audit entry emitted by the anonymizer for one applied operator.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OperatorResult {
    /// Entity type from the source [`RecognizerResult`].
    pub entity_type: EntityType,
    /// Operator variant that was applied.
    pub operator: crate::operator::OperatorKind,
    /// Inclusive start byte offset in the *original* text.
    pub original_start: usize,
    /// Exclusive end byte offset in the *original* text.
    pub original_end: usize,
    /// Inclusive start byte offset in the *anonymized* text.
    pub new_start: usize,
    /// Exclusive end byte offset in the *anonymized* text.
    pub new_end: usize,
}
