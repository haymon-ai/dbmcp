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

use dbmcp_config::PiiCategory;
use serde_json::Value;

use crate::Entity;
use crate::result::RecognizerResult;
use crate::words::push_key_words;
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

/// Error returned by [`Redactor::from_config`] when initialisation fails.
///
/// Surfacing this aborts server startup — the redactor is fail-closed: it
/// never starts in a degraded state.
#[derive(Debug, thiserror::Error)]
pub enum RedactorInitError {
    /// The optional NER engine failed to load; the server must not start.
    #[error("NER engine initialisation failed: {0}")]
    Ner(String),
}

/// Per-request redaction summary returned by [`Redactor::apply`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RedactionStats {
    /// Total spans rewritten across the request.
    pub total: u64,
    /// Per-entity-type counts; `BTreeMap` keeps tracing output stable.
    pub by_entity: BTreeMap<Entity, u64>,
    /// Number of `Value::String` leaves examined by the analyzer.
    ///
    /// Counts every leaf the walker reached, even ones that produced no
    /// PII spans. Operators can use it to distinguish "scanned 0 leaves"
    /// (e.g. row was a top-level number) from "scanned N, redacted 0"
    /// (no PII present).
    pub string_leaves_scanned: u64,
}

enum Frame<'a> {
    /// Top-level row or array element — no key words to push.
    Root(&'a mut Value),
    /// Object child — `key` is split into words on entry into the shared path.
    KeyedChild(&'a mut Value, &'a str),
    /// Truncates the shared path by `n` words once a subtree is done.
    Pop(usize),
}

/// Redacts PII from query tool response rows.
///
/// Holds an [`Arc<Analyzer>`] so handlers stay cheap to clone.
#[derive(Debug, Clone)]
pub struct Redactor {
    analyzer: Arc<Analyzer>,
    operator: OperatorConfig,
    opts: AnalyzeOptions,
}

impl Redactor {
    /// Builds a redactor with the [`Analyzer`]'s built-in recognizer set,
    /// the default operator, and the context-aware boost enabled with its
    /// documented defaults.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(Analyzer::with_defaults(), OperatorConfig::default())
    }

    #[cfg(test)]
    pub(crate) fn with_analyzer(analyzer: Analyzer) -> Self {
        Self::new(analyzer, OperatorConfig::default())
    }

    /// Builds a redactor wrapping a caller-supplied analyzer and operator config.
    ///
    /// The analyzer runs the context-aware boost with its documented
    /// defaults and the `min_score_with_context` floor.
    #[must_use]
    pub fn new(analyzer: Analyzer, operator: OperatorConfig) -> Self {
        let settings = crate::context::ContextSettings::default();
        let min_score = settings.min_score_with_context;
        Self {
            analyzer: Arc::new(analyzer),
            operator,
            opts: AnalyzeOptions {
                min_score,
                context: Some(settings),
            },
        }
    }

    /// Override the per-call [`AnalyzeOptions`] used by every leaf scan.
    ///
    /// Used by the binary layer to enable context-aware scoring per
    /// `PiiContextConfig`. Default (off) preserves today's behaviour.
    #[must_use]
    pub fn with_analyze_options(mut self, opts: AnalyzeOptions) -> Self {
        self.opts = opts;
        self
    }

    /// Resolve a [`dbmcp_config::PiiConfig`] to an optional [`Redactor`].
    ///
    /// Returns `None` when `cfg.enabled` is `false`. When enabled, the
    /// redactor runs the context-aware confidence boost over every leaf and
    /// drops candidates whose post-boost score falls below the
    /// `min_score_with_context` floor — so weak-pattern recognizers (CVV,
    /// AWS secret, bank account, …) surface only when a nearby keyword
    /// lifts them.
    /// # Errors
    ///
    /// Returns [`RedactorInitError`] when the optional NER engine is enabled
    /// but fails to load. Startup must abort — the redactor is fail-closed and
    /// never degrades to regex-only when NER was requested.
    pub fn from_config(cfg: &dbmcp_config::PiiConfig) -> Result<Option<Self>, RedactorInitError> {
        if !cfg.enabled {
            return Ok(None);
        }
        let mut analyzer = crate::Analyzer::from_config(cfg);
        attach_ner(&mut analyzer, cfg)?;
        Ok(Some(Self::new(analyzer, cfg.operator.into())))
    }

    /// Reports whether an ML/NER engine is attached.
    ///
    /// Callers use it to decide whether [`Self::apply`] needs offloading to
    /// a blocking thread.
    #[must_use]
    pub fn uses_ner(&self) -> bool {
        self.analyzer.ner_engine().is_some()
    }

    /// Walks every reachable string leaf in `rows` through the analyzer pipeline.
    ///
    /// Mutates `rows` in place. Recurses into [`Value::Object`] values
    /// and [`Value::Array`] elements at any depth using an iterative
    /// heap stack — call-stack depth does not scale with payload depth.
    /// Object keys are never inspected or modified; non-string scalars
    /// pass through unchanged. When an NER engine is attached, each leaf is
    /// also run through it and its spans merged with the regex hits. Emits
    /// one `tracing::info!` event per call when at least one span was
    /// rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Internal`] when the analyzer pipeline
    /// panics at any depth, or when the NER pass fails; the request must be
    /// failed without returning any row (fail-closed).
    pub fn apply(&self, rows: &mut [Value]) -> Result<RedactionStats, RedactionError> {
        let mut stats = RedactionStats::default();
        let mut infer_err: Option<String> = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Shared key-path stack. Each `Frame::KeyedChild` carries the tokens
            // to push when entered; a `Frame::Pop` queued before its children
            // restores the path after the subtree is done. This keeps path
            // mutations O(depth) instead of O(depth²) per leaf (no per-child
            // path clone).
            let mut path: Vec<String> = Vec::new();
            let mut stack: Vec<Frame<'_>> = rows.iter_mut().rev().map(Frame::Root).collect();
            while let Some(frame) = stack.pop() {
                let v = match frame {
                    Frame::Pop(n) => {
                        path.truncate(path.len() - n);
                        continue;
                    }
                    Frame::Root(v) => v,
                    Frame::KeyedChild(v, key) => {
                        // Pop pushed before children → runs after them (LIFO).
                        // Pop(0) is a no-op for separator-only keys.
                        let n = push_key_words(&mut path, key);
                        stack.push(Frame::Pop(n));
                        v
                    }
                };
                match v {
                    Value::String(s) => {
                        stats.string_leaves_scanned += 1;
                        let mut results = self.analyzer.analyze_with_context(s, &path, &self.opts);
                        if let Some(engine) = self.analyzer.ner_engine() {
                            match engine.analyze(s) {
                                Ok(ner) => results = merge_spans(results, ner, self.opts.min_score),
                                Err(e) => {
                                    infer_err = Some(e.to_string());
                                    return;
                                }
                            }
                        }
                        if results.is_empty() {
                            continue;
                        }
                        let anon = anonymize(s, results, &self.operator);
                        if anon.operations.is_empty() {
                            continue;
                        }
                        for op in &anon.operations {
                            stats.total += 1;
                            *stats.by_entity.entry(op.entity_type).or_insert(0) += 1;
                        }
                        *s = anon.text;
                    }
                    Value::Object(map) => {
                        for (key, child) in map.iter_mut() {
                            stack.push(Frame::KeyedChild(child, key));
                        }
                    }
                    Value::Array(arr) => {
                        // Array index does NOT inject context (sibling values
                        // are independent — FR-020 / spec edge case).
                        for child in arr.iter_mut() {
                            stack.push(Frame::Root(child));
                        }
                    }
                    Value::Number(_) | Value::Bool(_) | Value::Null => {}
                }
            }
        }));

        result.map_err(|_| RedactionError::Internal("analyzer panicked".into()))?;
        if let Some(e) = infer_err {
            return Err(RedactionError::Internal(format!("NER inference failed: {e}")));
        }
        log_redactions(&stats, rows.len());
        Ok(stats)
    }

    /// Redacts `rows`, keeping the heavier NER pass off the async runtime.
    ///
    /// Regex-only redaction runs inline on the caller's task; when an NER
    /// engine is attached the CPU-bound inference is moved to a blocking
    /// thread via [`tokio::task::spawn_blocking`] so it never stalls the
    /// async executor. Returns the (possibly rewritten) rows.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError`] when the analyzer pipeline fails at any
    /// depth or the offloaded task panics; the request must be failed
    /// without returning any row (fail-closed).
    pub async fn redact_rows(&self, mut rows: Vec<Value>) -> Result<Vec<Value>, RedactionError> {
        if self.uses_ner() {
            let redactor = self.clone();
            tokio::task::spawn_blocking(move || redactor.apply(&mut rows).map(|_| rows))
                .await
                .map_err(|e| RedactionError::Internal(format!("redaction task panicked: {e}")))?
        } else {
            self.apply(&mut rows)?;
            Ok(rows)
        }
    }
}

/// Emits one `tracing::info!` event when at least one span was rewritten.
fn log_redactions(stats: &RedactionStats, row_count: usize) {
    if stats.total > 0 {
        tracing::info!(
            target: "dbmcp::pii",
            redactions = stats.total,
            by_entity = ?stats.by_entity,
            rows = row_count,
            string_leaves_scanned = stats.string_leaves_scanned,
            "pii.redacted"
        );
    }
}

/// Loads the NER engine and attaches it, failing closed on any load error.
fn attach_ner(analyzer: &mut Analyzer, cfg: &dbmcp_config::PiiConfig) -> Result<(), RedactorInitError> {
    if !cfg.ner_enabled {
        return Ok(());
    }
    // NER respects the category filter via each entity's category. When the
    // category subset selects none of the NER entities, skip loading entirely.
    let allowed = allowed_ner_entities(cfg);
    if allowed.is_empty() {
        return Ok(());
    }
    let Some(model) = cfg.ner_model.as_ref() else {
        // `PiiConfig::validate` rejects this, but stay defensive and fail-closed.
        return Err(RedactorInitError::Ner("model path missing".to_owned()));
    };
    let threshold = crate::Score::from_static(dbmcp_config::PiiConfig::DEFAULT_NER_THRESHOLD);
    let mut engine =
        crate::ner::NerEngine::load(model, threshold).map_err(|e| RedactorInitError::Ner(e.to_string()))?;
    engine.set_allowed(allowed);
    analyzer.attach_ner(std::sync::Arc::new(engine));
    Ok(())
}

/// Category each NER-emitted entity belongs to; `None` for ones NER never emits.
fn ner_entity_category(entity: Entity) -> Option<PiiCategory> {
    match entity {
        Entity::Person | Entity::Organization | Entity::Nrp => Some(PiiCategory::Personal),
        Entity::Location | Entity::Facility => Some(PiiCategory::Contact),
        _ => None,
    }
}

/// Resolves the NER entities permitted by the category filter.
///
/// An unset category subset means all NER entities apply.
fn allowed_ner_entities(cfg: &dbmcp_config::PiiConfig) -> Vec<Entity> {
    let selected = cfg.categories.as_ref();
    crate::ner::NER_ENTITIES
        .iter()
        .copied()
        .filter(|&entity| match selected {
            None => true,
            Some(cats) => ner_entity_category(entity).is_some_and(|c| cats.contains(&c)),
        })
        .collect()
}

/// Merges regex and NER spans for one leaf into a resolved result set.
///
/// NER spans below `min_score` are dropped first; the combined set is then
/// overlap-resolved so higher-confidence spans win on collisions.
fn merge_spans(
    mut regex: Vec<RecognizerResult>,
    mut ner: Vec<RecognizerResult>,
    min_score: crate::Score,
) -> Vec<RecognizerResult> {
    ner.retain(|r| r.score >= min_score);
    regex.append(&mut ner);
    crate::overlap::resolve(regex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextSettings;
    use crate::pattern::Pattern;
    use crate::recognizers::Recognizer;
    use crate::score::Score;

    use crate::validators::Validator;
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
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress).copied(), Some(1));
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
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress).copied(), Some(2));
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
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress).copied(), Some(1));
        assert_eq!(stats.by_entity.get(&Entity::IpAddress).copied(), Some(1));
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
        let r = Redactor::new(Analyzer::with_defaults(), OperatorConfig::from(PiiOperator::Redact));
        let mut rows = vec![json!({"v": "x@y.com"})];
        let _ = r.apply(&mut rows).expect("apply ok");
        // Whole-leaf match under `redact` → "" (Value::String, key preserved).
        assert_eq!(rows[0]["v"], "");
        assert!(rows[0]["v"].is_string());
        assert!(rows[0].get("v").is_some());
    }

    #[test]
    fn whole_leaf_match_mask_matches_text_column() {
        let r = Redactor::new(Analyzer::with_defaults(), OperatorConfig::from(PiiOperator::Mask));
        let mut json_rows = vec![json!({"v": "x@y.com"})];
        let mut text_rows = vec![Value::String("x@y.com".to_owned())];
        let _ = r.apply(&mut json_rows).expect("apply ok");
        let _ = r.apply(&mut text_rows).expect("apply ok");
        assert_eq!(json_rows[0]["v"], text_rows[0]);
    }

    #[test]
    fn whole_leaf_match_hash_matches_text_column() {
        let r = Redactor::new(Analyzer::with_defaults(), OperatorConfig::from(PiiOperator::Hash));
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

    /// Build a rule whose validator panics on first invocation — used to
    /// exercise the fail-closed `catch_unwind` branch.
    fn panicking_rule() -> Recognizer {
        let regex = Pattern::new("anything", r".+", Score::from_static(0.9)).expect("static panic-rule regex compiles");
        Recognizer::new(Entity::EmailAddress, vec![regex])
            .expect("non-empty pattern list")
            .with_validator(Validator::Panic)
    }

    #[test]
    fn panicking_recognizer_surfaces_internal_error() {
        let mut analyzer = Analyzer::empty();
        analyzer.register(panicking_rule());
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
        analyzer.register(panicking_rule());
        let r = Redactor::with_analyzer(analyzer);
        // PII-bearing string lives 4 levels deep.
        let mut rows = vec![json!({"a": {"b": {"c": {"d": "anything"}}}})];
        let err = r.apply(&mut rows).expect_err("must fail-closed at any depth");
        match err {
            RedactionError::Internal(msg) => assert!(msg.contains("panicked")),
        }
    }

    fn ctx_settings() -> ContextSettings {
        ContextSettings {
            similarity_factor: Score::from_static(0.35),
            min_score_with_context: Score::from_static(0.4),
            prefix_words: 5,
            suffix_words: 0,
        }
    }

    fn weak_phone_analyzer() -> Analyzer {
        // Weak-confidence phone-shape pattern with NO validator (Validator::Noop)
        // so the candidate's score remains at 0.1 (below default min_score=0.4).
        // Context boost is the only path that can lift it to the floor.
        let p = Pattern::new("digits", r"\b\d{3} \d{3} \d{4}\b", Score::from_static(0.1)).expect("static");
        let rec = Recognizer::new(Entity::PhoneNumber, vec![p])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["phone"]);
        let mut a = Analyzer::empty();
        a.register(rec);
        a
    }

    #[test]
    fn redact_object_with_phone_column_boosts_via_key() {
        let r = Redactor::new(weak_phone_analyzer(), OperatorConfig::default()).with_analyze_options(AnalyzeOptions {
            min_score: Score::from_static(0.4),
            context: Some(ctx_settings()),
        });
        let mut rows = vec![json!({"customer_phone": "415 555 0142"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["customer_phone"], "<PHONE_NUMBER>");
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn redact_nested_object_accumulates_parent_keys() {
        let r = Redactor::new(weak_phone_analyzer(), OperatorConfig::default()).with_analyze_options(AnalyzeOptions {
            min_score: Score::from_static(0.4),
            context: Some(ctx_settings()),
        });
        let mut rows = vec![json!({"user": {"contact": {"phone_number": "415 555 0142"}}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["user"]["contact"]["phone_number"], "<PHONE_NUMBER>");
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn redact_array_does_not_leak_sibling_keys() {
        let r = Redactor::new(weak_phone_analyzer(), OperatorConfig::default()).with_analyze_options(AnalyzeOptions {
            min_score: Score::from_static(0.4),
            context: Some(ctx_settings()),
        });
        // Array of objects: each object has its own key path. A sibling
        // object's "phone" key MUST NOT seed context for the first object.
        let mut rows = vec![json!([
            {"name": "415 555 0142"},
            {"phone": "415 555 9999"}
        ])];
        let stats = r.apply(&mut rows).expect("apply ok");
        // First element: "name" doesn't match recognizer context → no boost → no redaction.
        assert_eq!(rows[0][0]["name"], "415 555 0142");
        // Second element: "phone" matches → boosted → redacted.
        assert_eq!(rows[0][1]["phone"], "<PHONE_NUMBER>");
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn redact_off_when_context_disabled_byte_identical() {
        // SC-002: with context disabled the redactor output is unchanged
        // from the no-context baseline.
        let r_off = Redactor::new(weak_phone_analyzer(), OperatorConfig::default());
        let mut rows = vec![json!({"customer_phone": "415 555 0142"})];
        let stats = r_off.apply(&mut rows).expect("apply ok");
        // No boost → candidate stays at score 0.1 → below 0.0 floor irrelevant,
        // but the analyzer's default min_score is 0.0, so the result IS emitted
        // with score 0.1. Redactor still rewrites it because anonymize replaces
        // every result regardless of score. Adjust expectation: with context
        // off there is no `supportive_keyword` and the rewrite is identical to
        // pre-feature output (the regex match itself triggered redaction).
        assert_eq!(rows[0]["customer_phone"], "<PHONE_NUMBER>");
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn dob_column_redacts_via_birth_keyword() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"date_of_birth": "2021-08-11"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["date_of_birth"], "<DATE_OF_BIRTH>");
        assert_eq!(stats.by_entity.get(&Entity::DateOfBirth).copied(), Some(1));
    }

    #[test]
    fn timestamp_column_not_flagged_as_date_of_birth() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"created_at": "2021-10-04"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["created_at"], "2021-10-04");
        assert!(!stats.by_entity.contains_key(&Entity::DateOfBirth));
    }

    #[test]
    fn zip_code_column_redacts_de_postcode_via_zip_keyword() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"zip_code": "41100"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["zip_code"], "<POSTCODE_DE>");
        assert_eq!(stats.by_entity.get(&Entity::PostcodeDe).copied(), Some(1));
    }

    #[test]
    fn de_postcode_value_untouched_without_address_context() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"reference": "41100"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["reference"], "41100");
        assert!(!stats.by_entity.contains_key(&Entity::PostcodeDe));
    }

    #[test]
    fn bcrypt_hash_redacted_value_only() {
        let r = Redactor::with_defaults();
        let hash = format!("$2y$12${}", "a".repeat(53));
        let mut rows = vec![json!({ "note": hash })];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["note"], "<PASSWORD_HASH>");
        assert_eq!(stats.by_entity.get(&Entity::PasswordHash).copied(), Some(1));
    }

    #[test]
    fn numeric_reference_untouched() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"reference": "900000000"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["reference"], "900000000");
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn from_config_disabled_is_none() {
        let cfg = dbmcp_config::PiiConfig::default();
        assert!(Redactor::from_config(&cfg).expect("ok").is_none());
    }

    fn rr(entity: Entity, start: usize, end: usize, score: f32) -> RecognizerResult {
        use crate::result::AnalysisExplanation;
        use crate::validation::ValidationOutcome;
        use std::borrow::Cow;
        let s = Score::from_static(score);
        RecognizerResult {
            entity_type: entity,
            start,
            end,
            score: s,
            explanation: AnalysisExplanation {
                recognizer_name: Cow::Borrowed("test"),
                pattern_name: None,
                original_score: s,
                validation: ValidationOutcome::Unknown,
                final_score: s,
                supportive_keyword: None,
            },
        }
    }

    #[test]
    fn merge_spans_drops_ner_below_min_score() {
        let ner = vec![rr(Entity::Person, 0, 4, 0.3)];
        let out = merge_spans(Vec::new(), ner, Score::from_static(0.5));
        assert!(out.is_empty(), "sub-threshold NER span must be dropped");
    }

    #[test]
    fn merge_spans_keeps_disjoint_regex_and_ner() {
        let regex = vec![rr(Entity::EmailAddress, 10, 25, 1.0)];
        let ner = vec![rr(Entity::Person, 0, 5, 0.9)];
        let out = merge_spans(regex, ner, Score::from_static(0.4));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn merge_spans_overlap_higher_score_wins() {
        // A checksum-strong regex hit (1.0) overlaps a weaker NER person guess.
        let regex = vec![rr(Entity::EmailAddress, 0, 16, 1.0)];
        let ner = vec![rr(Entity::Person, 0, 10, 0.6)];
        let out = merge_spans(regex, ner, Score::from_static(0.4));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entity_type, Entity::EmailAddress);
    }

    #[test]
    fn from_config_bad_model_path_fails_closed() {
        let cfg = dbmcp_config::PiiConfig {
            enabled: true,
            ner_enabled: true,
            ner_model: Some(std::path::PathBuf::from("/nonexistent/model/dir")),
            ..dbmcp_config::PiiConfig::default()
        };
        let err = Redactor::from_config(&cfg).expect_err("unreadable model must fail closed");
        assert!(matches!(err, RedactorInitError::Ner(_)));
    }

    #[test]
    fn ner_allowance_unset_categories_allows_all() {
        let cfg = dbmcp_config::PiiConfig {
            ner_enabled: true,
            ..dbmcp_config::PiiConfig::default()
        };
        let allowed = allowed_ner_entities(&cfg);
        assert_eq!(allowed.len(), 5, "unset categories must allow every NER entity");
        for entity in [
            Entity::Person,
            Entity::Location,
            Entity::Organization,
            Entity::Nrp,
            Entity::Facility,
        ] {
            assert!(allowed.contains(&entity), "{entity} must be allowed");
        }
    }

    #[test]
    fn ner_allowance_personal_only_gates_to_personal_entities() {
        let cfg = dbmcp_config::PiiConfig {
            categories: Some(vec![dbmcp_config::PiiCategory::Personal]),
            ..dbmcp_config::PiiConfig::default()
        };
        let allowed = allowed_ner_entities(&cfg);
        assert!(allowed.contains(&Entity::Person));
        assert!(allowed.contains(&Entity::Organization));
        assert!(allowed.contains(&Entity::Nrp));
        assert!(!allowed.contains(&Entity::Location));
        assert!(!allowed.contains(&Entity::Facility));
    }

    #[test]
    fn ner_allowance_contact_only_gates_to_contact_entities() {
        let cfg = dbmcp_config::PiiConfig {
            categories: Some(vec![dbmcp_config::PiiCategory::Contact]),
            ..dbmcp_config::PiiConfig::default()
        };
        let allowed = allowed_ner_entities(&cfg);
        assert!(allowed.contains(&Entity::Location));
        assert!(allowed.contains(&Entity::Facility));
        assert!(!allowed.contains(&Entity::Person));
        assert!(!allowed.contains(&Entity::Organization));
        assert!(!allowed.contains(&Entity::Nrp));
    }

    #[test]
    fn ner_allowance_unrelated_category_is_empty() {
        let cfg = dbmcp_config::PiiConfig {
            categories: Some(vec![dbmcp_config::PiiCategory::Financial]),
            ..dbmcp_config::PiiConfig::default()
        };
        assert!(
            allowed_ner_entities(&cfg).is_empty(),
            "a non-NER category must allow no NER entities (model not loaded)"
        );
    }
}
