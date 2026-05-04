//! PII redaction for query tool response payloads.
//!
//! Walks every reachable [`Value::String`] leaf in each row through the
//! [`Analyzer`] plus the configured per-entity operator (default
//! `Replace { "<TYPE>" }`), mutating the input slice in place. Object
//! keys, non-string scalars (`Number`, `Bool`, `Null`), and the JSON
//! shape (container ordering, key names, array indexes) are preserved
//! verbatim. The traversal is iterative — it uses an explicit
//! heap-resident stack of `&mut Value` work items, so deeply nested
//! payloads never blow the call stack.
//!
//! Failure mode is fail-closed at request granularity: any panic from
//! the analyzer pipeline at any depth is caught and surfaced as
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
    /// Number of `Value::String` leaves examined by the analyzer.
    ///
    /// Counts every leaf the walker reached, even ones that produced no
    /// PII spans. Operators can use it to distinguish "scanned 0 leaves"
    /// (e.g. row was a top-level number) from "scanned N, redacted 0"
    /// (no PII present).
    pub string_leaves_scanned: u64,
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

    /// Builds a redactor with default analyzer and a caller-chosen operator config.
    #[must_use]
    pub fn with_operator_config(operator: OperatorConfig) -> Self {
        Self {
            analyzer: Arc::new(Analyzer::with_defaults()),
            operator,
        }
    }

    /// Walks every reachable string leaf in `rows` through the analyzer pipeline.
    ///
    /// Mutates `rows` in place. Recurses into [`Value::Object`] values
    /// and [`Value::Array`] elements at any depth using an iterative
    /// heap stack — call-stack depth does not scale with payload depth.
    /// Object keys are never inspected or modified; non-string scalars
    /// pass through unchanged. Emits one `tracing::info!` event per
    /// call when at least one span was rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Internal`] when the analyzer pipeline
    /// panics at any depth; the request must be failed without
    /// returning any row.
    ///
    /// # Panics
    ///
    /// Does not panic in practice: the only `expect` call is on
    /// `serde_json::to_string` of a `BTreeMap<String, u64>`, which is
    /// infallible.
    pub fn apply(&self, rows: &mut [Value]) -> Result<RedactionStats, RedactionError> {
        let mut stats = RedactionStats::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut stack: Vec<&mut Value> = rows.iter_mut().collect();
            while let Some(v) = stack.pop() {
                match v {
                    Value::String(s) => {
                        stats.string_leaves_scanned += 1;
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
                        *s = anon.text;
                    }
                    Value::Object(map) => stack.extend(map.values_mut()),
                    Value::Array(arr) => stack.extend(arr.iter_mut()),
                    Value::Number(_) | Value::Bool(_) | Value::Null => {}
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
                rows = rows.len(),
                string_leaves_scanned = stats.string_leaves_scanned,
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
    use dbmcp_config::PiiOperator;
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
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn redacts_strings_inside_nested_array_and_object() {
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
        assert_eq!(rows[0]["arr"], json!(["<EMAIL_ADDRESS>"]));
        assert_eq!(rows[0]["obj"], json!({"k": "<EMAIL_ADDRESS>"}));
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_entity.get("EMAIL_ADDRESS").copied(), Some(2));
        assert_eq!(stats.string_leaves_scanned, 2);
    }

    #[test]
    fn redacts_email_at_depth_1_inside_array() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"emails": ["a@b.com", "c@d.com"]})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"emails": ["<EMAIL_ADDRESS>", "<EMAIL_ADDRESS>"]}));
        assert_eq!(stats.total, 2);
        assert_eq!(stats.string_leaves_scanned, 2);
    }

    #[test]
    fn redacts_email_at_depth_4_inside_chained_objects() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"a": {"b": {"c": {"d": "x@y.com"}}}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"a": {"b": {"c": {"d": "<EMAIL_ADDRESS>"}}}}));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn mixed_array_only_strings_with_pii_rewritten() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!([42, "a@b.com", true, null, {"ip": "1.2.3.4"}])];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0][0], 42);
        assert_eq!(rows[0][1], "<EMAIL_ADDRESS>");
        assert_eq!(rows[0][2], true);
        assert!(rows[0][3].is_null());
        assert_eq!(rows[0][4], json!({"ip": "<IP_ADDRESS>"}));
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_entity.get("EMAIL_ADDRESS").copied(), Some(1));
        assert_eq!(stats.by_entity.get("IP_ADDRESS").copied(), Some(1));
        assert_eq!(stats.string_leaves_scanned, 2);
    }

    #[test]
    fn top_level_array_row_walked() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!(["a@b.com"])];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!(["<EMAIL_ADDRESS>"]));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn top_level_string_row_walked() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!("a@b.com")];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!("<EMAIL_ADDRESS>"));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn empty_containers_pass_through_unchanged() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({}), json!([]), json!({"k": []}), json!({"k": {}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({}));
        assert_eq!(rows[1], json!([]));
        assert_eq!(rows[2], json!({"k": []}));
        assert_eq!(rows[3], json!({"k": {}}));
        assert_eq!(stats.total, 0);
        assert_eq!(stats.string_leaves_scanned, 0);
    }

    #[test]
    fn non_string_scalars_unchanged() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({
            "n": 42,
            "f": 2.71,
            "b": false,
            "z": null,
            "arr": [1, 2.0, true, null],
            "deep": {"x": [{"y": 7}]},
        })];
        let original = rows.clone();
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows, original);
        assert_eq!(stats.total, 0);
        assert_eq!(stats.string_leaves_scanned, 0);
    }

    #[test]
    fn deep_50000_nested_object_no_overflow() {
        let r = Redactor::with_defaults();
        let mut v = Value::String("x".to_owned());
        for _ in 0..50_000 {
            let mut map = serde_json::Map::new();
            map.insert("a".to_owned(), v);
            v = Value::Object(map);
        }
        let mut rows = vec![v];
        // Either Ok(_) (redacted/no-PII) or Err(Internal) acceptable per SC-005.
        // What MUST NOT happen: process crash or stack overflow inside `apply`.
        let outcome = r.apply(&mut rows);

        // serde_json's derived `Drop for Value` walks recursively; flatten
        // before scope exit so the deep tree drops level-by-level (each
        // intermediate `Map` is left empty by the `remove` call below, so its
        // own `Drop` is shallow).
        let mut head = rows.pop().expect("one row");
        while let Value::Object(ref mut m) = head {
            let Some(child) = m.remove("a") else { break };
            head = child;
        }

        match outcome {
            Ok(stats) => {
                assert_eq!(stats.total, 0);
                assert_eq!(stats.string_leaves_scanned, 1);
            }
            Err(RedactionError::Internal(_)) => {}
        }
    }

    #[test]
    fn string_leaves_scanned_counts_correctly() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({
            "s1": "one",
            "s2": "two",
            "n": 1,
            "arr": ["three", "four"],
            "nested": {"s5": "five"},
        })];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(stats.total, 0);
        assert_eq!(stats.string_leaves_scanned, 5);
        assert!(stats.string_leaves_scanned >= stats.total);
    }

    #[test]
    fn stats_total_invariant_holds_across_nested_payload() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({
            "user": {"email": "a@b.com", "ip": "1.2.3.4"},
            "log": ["c@d.com", "no-pii"],
        })];
        let stats = r.apply(&mut rows).expect("apply ok");
        let summed: u64 = stats.by_entity.values().sum();
        assert_eq!(stats.total, summed);
        assert!(stats.string_leaves_scanned >= stats.total);
        assert_eq!(stats.total, 3);
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
    fn same_pii_string_as_key_and_value_only_value_redacted() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"a@b.com": "a@b.com"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"a@b.com": "<EMAIL_ADDRESS>"}));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
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
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn flat_string_top_level_path_unchanged() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"email": "a@b.com", "age": 42})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"email": "<EMAIL_ADDRESS>", "age": 42}));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    #[test]
    fn whole_leaf_match_replace_emits_placeholder_token() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"v": "x@y.com"})];
        let _ = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["v"], "<EMAIL_ADDRESS>");
    }

    #[test]
    fn whole_leaf_match_redact_emits_empty_string() {
        let r = Redactor::with_operator_config(OperatorConfig::from(PiiOperator::Redact));
        let mut rows = vec![json!({"v": "x@y.com"})];
        let _ = r.apply(&mut rows).expect("apply ok");
        // Whole-leaf match under `redact` → "" (Value::String, key preserved).
        assert_eq!(rows[0]["v"], "");
        assert!(rows[0]["v"].is_string());
        assert!(rows[0].get("v").is_some());
    }

    #[test]
    fn whole_leaf_match_mask_matches_text_column() {
        let r = Redactor::with_operator_config(OperatorConfig::from(PiiOperator::Mask));
        let mut json_rows = vec![json!({"v": "x@y.com"})];
        let mut text_rows = vec![Value::String("x@y.com".to_owned())];
        let _ = r.apply(&mut json_rows).expect("apply ok");
        let _ = r.apply(&mut text_rows).expect("apply ok");
        assert_eq!(json_rows[0]["v"], text_rows[0]);
    }

    #[test]
    fn whole_leaf_match_hash_matches_text_column() {
        let r = Redactor::with_operator_config(OperatorConfig::from(PiiOperator::Hash));
        let mut json_rows = vec![json!({"v": "x@y.com"})];
        let mut text_rows = vec![Value::String("x@y.com".to_owned())];
        let _ = r.apply(&mut json_rows).expect("apply ok");
        let _ = r.apply(&mut text_rows).expect("apply ok");
        assert_eq!(json_rows[0]["v"], text_rows[0]);
    }

    #[test]
    fn mixed_row_redacts_text_and_json_columns_consistently() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({
            "text_col": "a@b.com",
            "json_col": {"email": "a@b.com"},
        })];
        let _ = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["text_col"], rows[0]["json_col"]["email"]);
        assert_eq!(rows[0]["text_col"], "<EMAIL_ADDRESS>");
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

    #[test]
    fn panic_at_depth_propagates_internal_error() {
        let mut analyzer = Analyzer::empty();
        analyzer.register(Box::new(PanickingRecognizer));
        let r = Redactor::with_analyzer(analyzer);
        // PII-bearing string lives 4 levels deep.
        let mut rows = vec![json!({"a": {"b": {"c": {"d": "anything"}}}})];
        let err = r.apply(&mut rows).expect_err("must fail-closed at any depth");
        match err {
            RedactionError::Internal(msg) => assert!(msg.contains("panicked")),
        }
    }
}
