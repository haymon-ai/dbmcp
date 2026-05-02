//! PII redaction for query tool response payloads.
//!
//! Walks each row's top-level string scalars through the [`Analyzer`]
//! plus the default per-entity operator (`Replace { "<TYPE>" }`),
//! mutating the input slice in place. Object keys, non-string values,
//! and nested structures are passed through verbatim.
//!
//! Failure mode is fail-closed at request granularity: any panic from
//! the analyzer pipeline is caught and surfaced as
//! [`RedactionError::Internal`], so no rows leak to the client when the
//! pipeline misbehaves. One `tracing::info!` event with target
//! `dbmcp::pii` is emitted per [`Redactor::apply`] call when at least
//! one span was rewritten.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use serde_json::Value;

use crate::{AnalyzeOptions, Analyzer, OperatorConfig, anonymize};

/// Errors produced by [`Redactor::apply`].
#[derive(Debug, thiserror::Error)]
pub enum RedactionError {
    /// Caught panic from the analyzer or anonymizer pipeline.
    #[error("PII redaction internal failure: {0}")]
    Internal(String),
}

impl From<RedactionError> for rmcp::model::ErrorData {
    fn from(e: RedactionError) -> Self {
        Self::internal_error(e.to_string(), None)
    }
}

/// Per-request redaction summary returned by [`Redactor::apply`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RedactionStats {
    /// Total spans rewritten across the request.
    pub total: u64,
    /// Per-entity-type counts; `BTreeMap` keeps tracing output stable.
    pub by_entity: BTreeMap<String, u64>,
    /// Number of `Value::Object` rows iterated.
    pub rows_scanned: u64,
}

/// Redacts PII from query tool response rows.
///
/// Holds an [`Arc<Analyzer>`] so handlers stay cheap to clone.
#[derive(Debug, Clone)]
pub struct Redactor {
    analyzer: Arc<Analyzer>,
    operator: OperatorConfig,
}

impl Redactor {
    /// Builds a redactor with the [`Analyzer`]'s built-in recognizer set.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self {
            analyzer: Arc::new(Analyzer::with_defaults()),
            operator: OperatorConfig::default(),
        }
    }

    /// Builds a redactor wrapping the supplied analyzer (test-only constructor).
    #[must_use]
    pub fn with_analyzer(analyzer: Analyzer) -> Self {
        Self {
            analyzer: Arc::new(analyzer),
            operator: OperatorConfig::default(),
        }
    }

    /// Walks every row's top-level string values through the analyzer pipeline.
    ///
    /// Mutates `rows` in place. Object keys are never touched; non-string
    /// values pass through. Emits one `tracing::info!` event per call when
    /// at least one span was rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Internal`] when the analyzer pipeline
    /// panics; the request must be failed without returning any row.
    ///
    /// # Panics
    ///
    /// Does not panic in practice: the only `expect` call is on
    /// `serde_json::to_string` of a `BTreeMap<String, u64>`, which is
    /// infallible.
    pub fn apply(&self, rows: &mut [Value]) -> Result<RedactionStats, RedactionError> {
        let mut stats = RedactionStats::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            for row in rows.iter_mut() {
                let Some(obj) = row.as_object_mut() else {
                    continue;
                };
                stats.rows_scanned += 1;
                for (_key, value) in obj.iter_mut() {
                    let Some(s) = value.as_str() else {
                        continue;
                    };
                    let results = self.analyzer.analyze(s, &AnalyzeOptions::default());
                    if results.is_empty() {
                        continue;
                    }
                    let anon = anonymize(s, results, &self.operator);
                    if anon.operations.is_empty() {
                        continue;
                    }
                    for op in &anon.operations {
                        stats.total += 1;
                        *stats.by_entity.entry(op.entity_type.as_str().to_owned()).or_default() += 1;
                    }
                    *value = Value::String(anon.text);
                }
            }
        }));

        result.map_err(|_| RedactionError::Internal("analyzer panicked".into()))?;

        if stats.total > 0 {
            let by_entity = serde_json::to_string(&stats.by_entity).expect("BTreeMap of String/u64 always serialises");
            tracing::info!(
                target: "dbmcp::pii",
                redactions = stats.total,
                by_entity = %by_entity,
                rows_scanned = stats.rows_scanned,
                "pii.redacted"
            );
        }

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityType, Recognizer, RecognizerResult};
    use serde_json::json;

    fn email_row() -> Value {
        json!({"msg": "ping me at jane.doe@example.com"})
    }

    #[test]
    fn rewrites_email_in_string_value() {
        let r = Redactor::with_defaults();
        let mut rows = vec![email_row()];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["msg"], "ping me at <EMAIL_ADDRESS>");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.by_entity.get("EMAIL_ADDRESS").copied(), Some(1));
        assert_eq!(stats.rows_scanned, 1);
    }

    #[test]
    fn passes_through_non_string_values() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({
            "n": 42,
            "flag": true,
            "missing": null,
            "arr": ["jane.doe@example.com"],
            "obj": {"k": "jane.doe@example.com"},
        })];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["n"], 42);
        assert_eq!(rows[0]["flag"], true);
        assert!(rows[0]["missing"].is_null());
        assert_eq!(rows[0]["arr"], json!(["jane.doe@example.com"]));
        assert_eq!(rows[0]["obj"], json!({"k": "jane.doe@example.com"}));
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn preserves_pii_shaped_keys() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"jane.doe@example.com": 1})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"jane.doe@example.com": 1}));
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn empty_input_returns_default_stats() {
        let r = Redactor::with_defaults();
        let mut rows: Vec<Value> = vec![];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(stats, RedactionStats::default());
    }

    #[test]
    fn no_match_does_not_mutate() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"msg": "order #1234 shipped"})];
        let original = rows.clone();
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows, original);
        assert_eq!(stats.total, 0);
    }

    /// Custom recognizer that panics on first analyze call — used to exercise
    /// the fail-closed `catch_unwind` branch.
    #[derive(Debug)]
    struct PanickingRecognizer;

    impl Recognizer for PanickingRecognizer {
        fn name(&self) -> &'static str {
            "panicking_test_recognizer"
        }
        fn supported_entities(&self) -> &[EntityType] {
            &[]
        }
        fn analyze(&self, _text: &str, _opts: &AnalyzeOptions) -> Vec<RecognizerResult> {
            panic!("intentional test panic");
        }
    }

    #[test]
    fn panicking_recognizer_surfaces_internal_error() {
        let mut analyzer = Analyzer::empty();
        analyzer.register(Box::new(PanickingRecognizer));
        let r = Redactor::with_analyzer(analyzer);
        let mut rows = vec![json!({"msg": "anything"})];
        let err = r.apply(&mut rows).expect_err("must fail-closed");
        match err {
            RedactionError::Internal(msg) => assert!(msg.contains("panicked")),
        }
    }
}
