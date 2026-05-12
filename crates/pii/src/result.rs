//! Typed PII span returned by recognizers and the analyzer engine.

use std::borrow::Cow;

use crate::score::Score;
use crate::{Entity, ValidationOutcome};

/// One detected PII span with audit trail.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecognizerResult {
    /// Entity type emitted by the recognizer.
    pub entity_type: Entity,
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
    /// Regex name that matched (`None` for non-regex recognizers, future use).
    pub pattern_name: Option<Cow<'static, str>>,
    /// Regex's base score, before validator adjustment.
    pub original_score: Score,
    /// Validator outcome applied to the candidate.
    pub validation: ValidationOutcome,
    /// Final score after validator adjustment; equals `RecognizerResult.score`.
    pub final_score: Score,
    /// Context keyword that triggered a confidence boost, when one fired.
    ///
    /// `None` when no boost was applied (feature disabled, no nearby keyword,
    /// already at [`crate::MAX_SCORE`], or recognizer carries no context list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supportive_keyword: Option<Cow<'static, str>>,
}

/// Audit entry emitted by the anonymizer for one applied operator.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OperatorResult {
    /// Entity type from the source [`RecognizerResult`].
    pub entity_type: Entity,
    /// Operator variant that was applied.
    pub operator: crate::operators::OperatorKind,
    /// Inclusive start byte offset in the *original* text.
    pub original_start: usize,
    /// Exclusive end byte offset in the *original* text.
    pub original_end: usize,
    /// Inclusive start byte offset in the *anonymized* text.
    pub new_start: usize,
    /// Exclusive end byte offset in the *anonymized* text.
    pub new_end: usize,
}

#[cfg(test)]
mod tests {
    use super::AnalysisExplanation;
    use crate::Entity;
    use crate::score::{MAX_SCORE, Score};
    use crate::validation::ValidationOutcome;
    use std::borrow::Cow;

    fn sample_explanation() -> AnalysisExplanation {
        AnalysisExplanation {
            recognizer_name: Cow::Borrowed("FooRecognizer"),
            pattern_name: Some(Cow::Borrowed("foo")),
            original_score: Score::from_static(0.3),
            validation: ValidationOutcome::Unknown,
            final_score: Score::from_static(0.3),
            supportive_keyword: None,
        }
    }

    #[test]
    fn default_explanation_has_no_supportive_keyword() {
        let exp = sample_explanation();
        let json = serde_json::to_string(&exp).expect("serialise");
        assert!(
            !json.contains("supportive_keyword"),
            "absent field must not appear in JSON: {json}"
        );
    }

    #[test]
    fn boosted_explanation_serialises_keyword() {
        let mut exp = sample_explanation();
        exp.supportive_keyword = Some(Cow::Borrowed("card"));
        exp.final_score = MAX_SCORE;
        let _ = Entity::CreditCard;
        let json = serde_json::to_string(&exp).expect("serialise");
        assert!(json.contains("\"supportive_keyword\":\"card\""), "got: {json}");
    }

    #[test]
    fn deserialise_without_field_yields_none() {
        let json = r#"{
            "recognizer_name": "X",
            "pattern_name": null,
            "original_score": 0.3,
            "validation": "Unknown",
            "final_score": 0.3
        }"#;
        let exp: AnalysisExplanation = serde_json::from_str(json).expect("deserialise");
        assert!(exp.supportive_keyword.is_none());
    }

    #[test]
    fn deserialise_with_field_yields_some() {
        let json = r#"{
            "recognizer_name": "X",
            "pattern_name": null,
            "original_score": 0.3,
            "validation": "Unknown",
            "final_score": 0.5,
            "supportive_keyword": "phone"
        }"#;
        let exp: AnalysisExplanation = serde_json::from_str(json).expect("deserialise");
        assert_eq!(exp.supportive_keyword.as_deref(), Some("phone"));
    }
}
