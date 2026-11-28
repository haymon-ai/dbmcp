//! PII redaction for query tool response payloads.
//!
//! Walks every reachable [`Value::String`] leaf in each row through the
//! [`Analyzer`] plus the configured [`Operator`], mutating the input slice
//! in place. Object keys inside a value are scanned too (regex only, with
//! the parent key path as context) and their detected spans rewritten by
//! the same operator; colliding sibling keys take `#2`, `#3`, … suffixes,
//! and a key `redact` would empty becomes the entity placeholder. Row-level
//! keys (column names), non-string scalars (`Number`, `Bool`, `Null`), and
//! the JSON shape (container ordering, array indexes) are preserved
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

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use dbmcp_types::privacy;
use secrecy::ExposeSecret;
use serde_json::{Map, Value};

use crate::Entity;
use crate::result::RecognizerResult;
use crate::words::push_key_words;
use crate::{AnalyzeOptions, Analyzer, Operator, anonymize};

/// Errors produced by [`Redactor::apply`].
#[derive(Debug, thiserror::Error)]
pub enum RedactionError {
    /// Caught panic from the analyzer or anonymizer pipeline.
    #[error("PII redaction internal failure: {0}")]
    Internal(String),
    /// Result columns do not fit the column plan.
    #[error(transparent)]
    Shape(#[from] crate::cells::ShapeError),
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
    /// The OS random source failed while drawing the fallback `hash` key.
    #[error("failed to generate a random privacy key: {0}")]
    Rng(getrandom::Error),
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
    /// Number of object keys renamed because they held at least one span.
    ///
    /// Their spans count toward [`Self::total`] and [`Self::by_entity`].
    pub keys_rewritten: u64,
}

pub(crate) enum Frame<'a> {
    /// Result row — its own keys are column names and are never renamed.
    Row(&'a mut Value),
    /// Array element — no key words to push.
    Root(&'a mut Value),
    /// Object child — `key`, the original text before any rename, is split
    /// into words on entry into the shared path.
    KeyedChild(&'a mut Value, Cow<'a, str>),
    /// Truncates the shared path by `n` words once a subtree is done.
    Pop(usize),
}

/// One string leaf deferred to the batched NER pass.
///
/// `slot` is the write-back handle to the (now-empty) leaf; `text` owns the
/// leaf's original contents, moved out to serve as batch input; `regex` holds
/// its already-computed regex spans. The merge pass restores `text` into `slot`.
struct LeafSlot<'a> {
    slot: &'a mut String,
    text: String,
    regex: Vec<RecognizerResult>,
}

/// Redacts PII from query tool response rows.
///
/// Holds an [`Arc<Analyzer>`] so handlers stay cheap to clone.
#[derive(Debug, Clone)]
pub struct Redactor {
    /// Global detection; `None` under `rules` mode, where only ruled columns change.
    analyzer: Option<Arc<Analyzer>>,
    /// The operator global detection applies to every span.
    global: privacy::Operator,
    /// The keyed `hash` operator of global detection.
    hash: Operator,
    /// The `hash` operator of column rules, under a key derived for rules only.
    ///
    /// A separate key keeps a query from hashing a value of its own choosing
    /// through global detection and matching a ruled column's token.
    rule_hash: Operator,
    opts: AnalyzeOptions,
}

/// The fixed `hash` key of test redactors, so tokens are deterministic.
#[cfg(test)]
const TEST_KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

impl Redactor {
    /// Builds a `Replace` redactor over the built-in recognizer set.
    #[cfg(test)]
    pub(crate) fn with_defaults() -> Self {
        Self::with_analyzer(Analyzer::with_defaults())
    }

    /// Builds a `Replace` redactor over `analyzer`.
    #[cfg(test)]
    pub(crate) fn with_analyzer(analyzer: Analyzer) -> Self {
        Self::new(Some(analyzer), privacy::Operator::Replace, TEST_KEY)
    }

    /// Builds a redactor whose `hash` operators derive from `key`.
    ///
    /// The analyzer runs the context-aware boost with its documented
    /// defaults and the `min_score_with_context` floor.
    fn new(analyzer: Option<Analyzer>, global: privacy::Operator, key: &[u8]) -> Self {
        let settings = crate::context::ContextSettings::default();
        let min_score = settings.min_score_with_context;
        Self {
            analyzer: analyzer.map(Arc::new),
            global,
            hash: Operator::hash(key),
            rule_hash: Operator::rule_hash(key),
            opts: AnalyzeOptions {
                min_score,
                context: Some(settings),
            },
        }
    }

    /// Override the per-call [`AnalyzeOptions`] used by every leaf scan.
    ///
    /// Used by the binary layer to enable context-aware scoring per
    /// `PrivacyContextConfig`. Default (off) preserves today's behaviour.
    #[must_use]
    pub fn with_analyze_options(mut self, opts: AnalyzeOptions) -> Self {
        self.opts = opts;
        self
    }

    /// Resolve a [`privacy::Config`] to an optional [`Redactor`].
    ///
    /// Returns `None` when `cfg` is `None` (mode `none`). Under `rules` it
    /// builds no analyzer: ruled columns are rewritten, global cells and error
    /// text pass through. Under `full`, the redactor runs the context-aware confidence boost over every leaf and
    /// drops candidates whose post-boost score falls below the
    /// `min_score_with_context` floor — so weak-pattern recognizers (CVV,
    /// AWS secret, bank account, …) surface only when a nearby keyword
    /// lifts them.
    ///
    /// The `hash` operator is keyed from `cfg.key`, or, when unset, from
    /// [`privacy::Config::MIN_KEY_BYTES`]
    /// bytes drawn from the OS random source (a per-process key), logging a
    /// warning when `cfg.operator` or any column rule hashes. Column rules
    /// hash under a key derived from it for rules only, so their tokens never
    /// equal global detection's.
    ///
    /// # Errors
    ///
    /// Returns [`RedactorInitError::Ner`] when `full` sets a model that fails
    /// to load, and [`RedactorInitError::Rng`] when the
    /// fallback `hash` key cannot be drawn. Startup must abort — the redactor
    /// is fail-closed and never degrades to regex-only when NER was requested.
    pub fn from_config(cfg: Option<&privacy::Config>) -> Result<Option<Self>, RedactorInitError> {
        let Some(cfg) = cfg else {
            return Ok(None);
        };
        let random;
        let key = if let Some(key) = &cfg.key {
            key.expose_secret().as_bytes()
        } else {
            random = random_key(cfg)?;
            &random[..]
        };
        let analyzer = match cfg.mode {
            privacy::Mode::Full => {
                let mut analyzer = crate::Analyzer::from_config(cfg);
                attach_ner(&mut analyzer, cfg)?;
                Some(analyzer)
            }
            privacy::Mode::Rules | privacy::Mode::None => None,
        };
        Ok(Some(Self::new(analyzer, cfg.operator, key)))
    }

    /// Returns global detection's operator `operator`; `hash` uses its key.
    pub(crate) fn operator(&self, operator: privacy::Operator) -> Operator {
        match operator {
            privacy::Operator::Replace => Operator::Replace,
            privacy::Operator::Mask => Operator::default_mask(),
            privacy::Operator::Redact => Operator::Redact,
            privacy::Operator::Hash => self.hash.clone(),
        }
    }

    /// Returns a column rule's operator `operator`; `hash` uses the rules' key.
    pub(crate) fn rule_operator(&self, operator: privacy::Operator) -> Operator {
        match operator {
            privacy::Operator::Hash => self.rule_hash.clone(),
            other => self.operator(other),
        }
    }

    /// Reports whether an ML/NER engine is attached.
    ///
    /// Callers use it to decide whether [`Self::apply`] needs offloading to
    /// a blocking thread.
    #[must_use]
    pub fn uses_ner(&self) -> bool {
        self.analyzer
            .as_ref()
            .is_some_and(|analyzer| analyzer.ner_engine().is_some())
    }

    /// Walks every reachable string leaf in `rows` through the analyzer pipeline.
    ///
    /// Mutates `rows` in place. Recurses into [`Value::Object`] values
    /// and [`Value::Array`] elements at any depth using an iterative
    /// heap stack — call-stack depth does not scale with payload depth.
    /// Each row object's own keys (column names) are left as they are; keys
    /// below them are scanned with the regex recognizers and renamed where a
    /// span is found (see the module docs); non-string scalars pass through
    /// unchanged. When an NER engine is attached, each leaf is
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
        self.walk(rows.iter_mut().rev().map(Frame::Row).collect(), &mut stats)?;
        log_redactions(&stats, Some(rows.len()));
        Ok(stats)
    }

    /// Runs global detection over every string leaf under `roots`.
    ///
    /// `roots` is a stack: the last frame is walked first. A
    /// [`Frame::KeyedChild`] root contributes its key as context. Keys of
    /// every object below a root are renamed, except a [`Frame::Row`]'s own.
    ///
    /// # Errors
    ///
    /// See [`Self::apply`].
    pub(crate) fn walk(&self, roots: Vec<Frame<'_>>, stats: &mut RedactionStats) -> Result<(), RedactionError> {
        let Some(analyzer) = self.analyzer.as_deref() else {
            return Ok(());
        };
        let operator = self.operator(self.global);
        let mut infer_err: Option<String> = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            // When NER is attached, defer inference: collect every leaf during the
            // walk and run one batched forward pass afterwards, instead of one pass
            // per leaf. Without NER, anonymize inline (regex-only).
            let ner_engine = analyzer.ner_engine();
            let mut slots: Vec<LeafSlot<'_>> = Vec::new();

            // Shared key-path stack. Each `Frame::KeyedChild` carries the tokens
            // to push when entered; a `Frame::Pop` queued before its children
            // restores the path after the subtree is done. This keeps path
            // mutations O(depth) instead of O(depth²) per leaf (no per-child
            // path clone).
            let mut path: Vec<String> = Vec::new();
            let mut stack = roots;
            while let Some(frame) = stack.pop() {
                let (v, rename) = match frame {
                    Frame::Pop(n) => {
                        path.truncate(path.len() - n);
                        continue;
                    }
                    Frame::Row(v) => (v, false),
                    Frame::Root(v) => (v, true),
                    Frame::KeyedChild(v, key) => {
                        // Pop pushed before children → runs after them (LIFO).
                        // Pop(0) is a no-op for separator-only keys.
                        let n = push_key_words(&mut path, &key);
                        stack.push(Frame::Pop(n));
                        (v, true)
                    }
                };
                match v {
                    Value::String(s) => {
                        stats.string_leaves_scanned += 1;
                        let regex = analyzer.analyze_with_context(s, &path, &self.opts);
                        if ner_engine.is_some() {
                            // Move the leaf's text out as batch input (no clone),
                            // leaving the leaf empty; the merge pass restores it
                            // before rewriting. `s` stays as the write-back handle.
                            let text = std::mem::take(s);
                            slots.push(LeafSlot { slot: s, text, regex });
                        } else {
                            redact_spans(s, regex, Vec::new(), self.opts.min_score, &operator, stats);
                        }
                    }
                    Value::Object(map) => {
                        // Children keep the original key's words as context.
                        let mut originals = if rename {
                            self.rename_keys(map, analyzer, &path, &operator, stats)
                        } else {
                            Vec::new()
                        }
                        .into_iter();
                        for (key, child) in map.iter_mut() {
                            let key = originals
                                .next()
                                .flatten()
                                .map_or(Cow::Borrowed(key.as_str()), Cow::Owned);
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

            // On failure, record the error and leave the moved-out leaves empty:
            // `apply` fails closed and the caller discards the rows, so the
            // emptied (or partial) state never leaks.
            if let Some(engine) = ner_engine
                && !slots.is_empty()
            {
                let texts: Vec<&str> = slots.iter().map(|l| l.text.as_str()).collect();
                match engine.analyze_batch(&texts) {
                    Ok(per_leaf) => {
                        for (leaf, ner) in slots.into_iter().zip(per_leaf) {
                            // Restore the moved-out text into the emptied leaf, so a
                            // no-PII leaf keeps its original value and redact_spans
                            // rewrites in place.
                            *leaf.slot = leaf.text;
                            redact_spans(leaf.slot, leaf.regex, ner, self.opts.min_score, &operator, stats);
                        }
                    }
                    Err(e) => infer_err = Some(e.to_string()),
                }
            }
        }));

        result.map_err(|_| RedactionError::Internal("analyzer panicked".into()))?;
        if let Some(e) = infer_err {
            return Err(RedactionError::Internal(format!("NER inference failed: {e}")));
        }
        Ok(())
    }

    /// Renames every key of `map` holding a detected span, keeping map order.
    ///
    /// Keys are analyzed by the regex recognizers only, with `path` as
    /// context. A renamed key that collides with a sibling in the final key
    /// set takes the next free `#n` suffix (from `#2`), in map order. Returns
    /// each entry's original key where renamed, by position, or nothing when
    /// no key changed (the map is then left untouched).
    fn rename_keys(
        &self,
        map: &mut Map<String, Value>,
        analyzer: &Analyzer,
        path: &[String],
        operator: &Operator,
        stats: &mut RedactionStats,
    ) -> Vec<Option<String>> {
        let renamed: Vec<Option<String>> = map
            .keys()
            .map(|key| rename_key(key, analyzer, path, &self.opts, operator, stats))
            .collect();
        if renamed.iter().all(Option::is_none) {
            return Vec::new();
        }
        let mut taken: HashSet<String> = map
            .keys()
            .zip(&renamed)
            .filter(|(_, new)| new.is_none())
            .map(|(key, _)| key.clone())
            .collect();
        // Next suffix to try per renamed text, so `n` collisions cost O(n).
        let mut next = HashMap::new();
        let mut originals = Vec::with_capacity(renamed.len());
        for ((key, value), new) in std::mem::take(map).into_iter().zip(renamed) {
            let Some(new) = new else {
                map.insert(key, value);
                originals.push(None);
                continue;
            };
            let unique = free_key(&taken, &mut next, new);
            taken.insert(unique.clone());
            map.insert(unique, value);
            originals.push(Some(key));
        }
        originals
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

    /// Redacts a single free-text string, such as a tool error message.
    ///
    /// Runs only the regex recognizers, with no key-path context; under
    /// `rules` mode (no analyzer) the text comes back unchanged. The NER pass
    /// is skipped: without column context it rewrites table, column, and
    /// schema identifiers the caller needs to correct a failing query. Emits
    /// one `pii.redacted_text` event when at least one span was rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Internal`] when the analyzer pipeline panics;
    /// the caller must not fall back to the unredacted text (fail-closed).
    pub fn redact_text(&self, text: &str) -> Result<String, RedactionError> {
        let Some(analyzer) = self.analyzer.as_deref() else {
            return Ok(text.to_owned());
        };
        let mut stats = RedactionStats::default();
        let mut out = text.to_owned();
        let operator = self.operator(self.global);
        catch_unwind(AssertUnwindSafe(|| {
            let results = analyzer.analyze_with_context(&out, &[], &self.opts);
            redact_spans(
                &mut out,
                results,
                Vec::new(),
                self.opts.min_score,
                &operator,
                &mut stats,
            );
        }))
        .map_err(|_| RedactionError::Internal("analyzer panicked".into()))?;
        log_redactions(&stats, None);
        Ok(out)
    }
}

/// Redaction over the `Option<Redactor>` field every query handler holds.
///
/// Centralizes the "redact when enabled, pass through when not" branch so
/// each query tool calls one method instead of matching on the option.
#[allow(async_fn_in_trait)]
pub trait MaybeRedact {
    /// Redacts `rows` when a redactor is present, else returns them unchanged.
    ///
    /// # Errors
    ///
    /// Propagates [`RedactionError`] from [`Redactor::redact_rows`].
    async fn redact_rows(&self, rows: Vec<Value>) -> Result<Vec<Value>, RedactionError>;

    /// Redacts a free-text message when a redactor is present, else returns it unchanged.
    ///
    /// Fails closed: if redaction fails, the [`RedactionError`] text replaces
    /// the message so the unredacted text is never returned.
    fn redact_message(&self, message: &str) -> String;
}

impl MaybeRedact for Option<Redactor> {
    async fn redact_rows(&self, rows: Vec<Value>) -> Result<Vec<Value>, RedactionError> {
        match self {
            Some(r) => r.redact_rows(rows).await,
            None => Ok(rows),
        }
    }

    fn redact_message(&self, message: &str) -> String {
        match self {
            Some(r) => r.redact_text(message).unwrap_or_else(|e| e.to_string()),
            None => message.to_owned(),
        }
    }
}

/// Emits one `tracing::info!` event when at least one span was rewritten.
///
/// `row_count` is `Some` for row payloads (`pii.redacted`) and `None` for
/// free text such as error messages (`pii.redacted_text`).
pub(crate) fn log_redactions(stats: &RedactionStats, row_count: Option<usize>) {
    if stats.total == 0 {
        return;
    }
    if let Some(rows) = row_count {
        tracing::info!(
            target: "dbmcp::pii",
            redactions = stats.total,
            by_entity = ?stats.by_entity,
            rows,
            string_leaves_scanned = stats.string_leaves_scanned,
            keys_rewritten = stats.keys_rewritten,
            "pii.redacted"
        );
    } else {
        tracing::info!(
            target: "dbmcp::pii",
            redactions = stats.total,
            by_entity = ?stats.by_entity,
            "pii.redacted_text"
        );
    }
}

/// Loads the NER engine when a model is set, failing closed on any load error.
fn attach_ner(analyzer: &mut Analyzer, cfg: &privacy::Config) -> Result<(), RedactorInitError> {
    let Some(model) = cfg.model.as_ref() else {
        return Ok(());
    };
    // NER respects the category filter via each entity's category. When the
    // category subset selects none of the NER entities, skip loading entirely.
    let allowed = allowed_ner_entities(cfg);
    if allowed.is_empty() {
        return Ok(());
    }
    let threshold = crate::Score::from_static(privacy::Config::DEFAULT_NER_THRESHOLD);
    let mut engine =
        crate::ner::NerEngine::load(model, threshold).map_err(|e| RedactorInitError::Ner(e.to_string()))?;
    engine.set_allowed(allowed);
    analyzer.attach_ner(std::sync::Arc::new(engine));
    Ok(())
}

/// Category each NER-emitted entity belongs to; `None` for ones NER never emits.
fn ner_entity_category(entity: Entity) -> Option<privacy::Category> {
    match entity {
        Entity::Person | Entity::Organization | Entity::Nrp => Some(privacy::Category::Personal),
        Entity::Location | Entity::Facility => Some(privacy::Category::Contact),
        _ => None,
    }
}

/// Resolves the NER entities permitted by the category filter.
///
/// An unset category subset means all NER entities apply.
fn allowed_ner_entities(cfg: &privacy::Config) -> Vec<Entity> {
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

/// Rewrites `s` in place over its regex and NER spans, counting into `stats`.
///
/// NER spans below `min_score` are dropped (regex spans arrive filtered but
/// unresolved); the rest are overlap-resolved once, together, by [`anonymize`]
/// (see [`crate::overlap`]), so a regex span is never cut or relabeled by an
/// NER span. No-op when no span remains.
/// Shared by the regex-only leaf, the batched NER leaf, and free text.
fn redact_spans(
    s: &mut String,
    mut regex: Vec<RecognizerResult>,
    mut ner: Vec<RecognizerResult>,
    min_score: crate::Score,
    operator: &Operator,
    stats: &mut RedactionStats,
) {
    ner.retain(|r| r.score >= min_score);
    regex.append(&mut ner);
    if regex.is_empty() {
        return;
    }
    let anon = anonymize(s, regex, operator);
    if anon.operations.is_empty() {
        return;
    }
    for op in &anon.operations {
        stats.total += 1;
        *stats.by_entity.entry(op.entity_type).or_insert(0) += 1;
    }
    *s = anon.text;
}

/// Returns `key` rewritten over its regex spans, or `None` when it holds none.
///
/// A key the operator would empty (`redact` over the whole key) becomes the
/// first span's entity placeholder instead.
fn rename_key(
    key: &str,
    analyzer: &Analyzer,
    path: &[String],
    opts: &AnalyzeOptions,
    operator: &Operator,
    stats: &mut RedactionStats,
) -> Option<String> {
    let results = analyzer.analyze_with_context(key, path, opts);
    if results.is_empty() {
        return None;
    }
    let anon = anonymize(key, results, operator);
    let first = anon.operations.first()?.entity_type;
    stats.keys_rewritten += 1;
    for op in &anon.operations {
        stats.total += 1;
        *stats.by_entity.entry(op.entity_type).or_insert(0) += 1;
    }
    if anon.text.is_empty() {
        return Some(first.placeholder().to_owned());
    }
    Some(anon.text)
}

/// Returns `key`, or `key#n` for the lowest free `n` from `next[key]` (or 2).
///
/// Suffixes below `next[key]` were handed out before, so the search resumes
/// there and records the suffix after the one it returns.
fn free_key(taken: &HashSet<String>, next: &mut HashMap<String, usize>, key: String) -> String {
    if !taken.contains(&key) {
        return key;
    }
    let start = next.get(&key).copied().unwrap_or(2);
    // `taken.len() + 1` candidates cannot all be taken.
    let (n, candidate) = (start..=start + taken.len())
        .map(|n| (n, format!("{key}#{n}")))
        .find(|(_, candidate)| !taken.contains(candidate))
        .expect("more candidates than taken keys");
    next.insert(key, n + 1);
    candidate
}

/// Draws the random per-process `hash` key used when `privacy.key` is unset.
fn random_key(cfg: &privacy::Config) -> Result<[u8; privacy::Config::MIN_KEY_BYTES], RedactorInitError> {
    let mut key = [0u8; privacy::Config::MIN_KEY_BYTES];
    getrandom::fill(&mut key).map_err(RedactorInitError::Rng)?;
    if cfg.operator == privacy::Operator::Hash || cfg.rules.iter().any(|rule| rule.operator == privacy::Operator::Hash)
    {
        tracing::warn!(
            "privacy.key is not set; PII placeholders use a random per-process key and change on restart and across replicas"
        );
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextSettings;
    use crate::pattern::Pattern;
    use crate::recognizers::Recognizer;
    use crate::result::Detector;
    use crate::score::Score;

    use crate::validators::Validator;
    use serde_json::json;

    /// The 32-byte test key from the keyed-token spec vectors.
    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn hash_config(key: Option<&str>) -> privacy::Config {
        privacy::Config {
            operator: privacy::Operator::Hash,
            key: key.map(Into::into),
            ..privacy::Config::default()
        }
    }

    fn build(cfg: &privacy::Config) -> Redactor {
        Redactor::from_config(Some(cfg))
            .expect("config ok")
            .expect("redactor present")
    }

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
            let Some(child) = m.shift_remove("a") else { break };
            head = child;
        }

        match outcome {
            Ok(stats) => {
                assert_eq!(stats.total, 0);
                assert_eq!(stats.string_leaves_scanned, 1);
            }
            Err(e) => assert!(matches!(e, RedactionError::Internal(_)), "{e}"),
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
    fn renames_pii_shaped_keys_inside_a_value() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"doc": {"jane.doe@example.com": 1}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"doc": {"<EMAIL_ADDRESS>": 1}}));
        assert_eq!(stats.total, 1);
        assert_eq!(stats.keys_rewritten, 1);
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress).copied(), Some(1));
    }

    #[test]
    fn same_pii_string_as_key_and_value_both_redacted() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"doc": {"a@b.com": "a@b.com"}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"doc": {"<EMAIL_ADDRESS>": "<EMAIL_ADDRESS>"}}));
        assert_eq!(stats.total, 2);
        assert_eq!(stats.keys_rewritten, 1);
        assert_eq!(stats.string_leaves_scanned, 1);
    }

    /// A default-analyzer redactor under global `operator` and the test key.
    fn under(operator: privacy::Operator) -> Redactor {
        Redactor::new(Some(Analyzer::with_defaults()), operator, KEY.as_bytes())
    }

    /// Redacts one row holding `doc` under column `doc` and returns the cell.
    fn redact_doc(r: &Redactor, doc: Value) -> Value {
        let mut rows = vec![Value::Object(Map::from_iter([("doc".to_owned(), doc)]))];
        r.apply(&mut rows).expect("apply ok");
        rows.remove(0)["doc"].take()
    }

    /// The keys of a JSON object, in map order.
    fn keys(value: &Value) -> Vec<String> {
        value.as_object().expect("object").keys().cloned().collect()
    }

    #[test]
    fn renames_a_key_under_each_operator() {
        let doc = json!({"x@y.com": 1});
        let cases = [
            (privacy::Operator::Replace, "<EMAIL_ADDRESS>"),
            (privacy::Operator::Mask, "*******"),
            (privacy::Operator::Redact, "<EMAIL_ADDRESS>"),
            (privacy::Operator::Hash, "<EMAIL_ADDRESS_06fff299b93c0784>"),
        ];
        for (operator, key) in cases {
            assert_eq!(
                redact_doc(&under(operator), doc.clone()),
                json!({ key: 1 }),
                "{operator:?}"
            );
        }
    }

    #[test]
    fn a_hashed_key_matches_the_same_value_hashed() {
        let out = redact_doc(&under(privacy::Operator::Hash), json!({"x@y.com": "x@y.com"}));
        let (key, value) = out.as_object().expect("object").iter().next().expect("one entry");
        assert_eq!(value, key.as_str());
        assert_eq!(key, "<EMAIL_ADDRESS_06fff299b93c0784>");
    }

    #[test]
    fn colliding_renamed_keys_take_ordinal_suffixes() {
        let doc = json!({"a@b.com": 1, "c@d.com": 2, "e@f.com": 3});
        let cases = [
            (privacy::Operator::Replace, "<EMAIL_ADDRESS>"),
            (privacy::Operator::Mask, "*******"),
            (privacy::Operator::Redact, "<EMAIL_ADDRESS>"),
        ];
        for (operator, key) in cases {
            let out = redact_doc(&under(operator), doc.clone());
            assert_eq!(
                keys(&out),
                [key.to_owned(), format!("{key}#2"), format!("{key}#3")],
                "{operator:?}"
            );
            assert_eq!(out.as_object().expect("object").values().collect::<Vec<_>>(), [1, 2, 3]);
        }
    }

    #[test]
    fn renamed_keys_avoid_literal_siblings() {
        let r = Redactor::with_defaults();
        let out = redact_doc(
            &r,
            json!({"a@b.com": 1, "<EMAIL_ADDRESS>": 2, "<EMAIL_ADDRESS>#2": 3, "c@d.com": 4}),
        );
        assert_eq!(
            out,
            json!({"<EMAIL_ADDRESS>#3": 1, "<EMAIL_ADDRESS>": 2, "<EMAIL_ADDRESS>#2": 3, "<EMAIL_ADDRESS>#4": 4})
        );
        assert_eq!(
            keys(&out),
            [
                "<EMAIL_ADDRESS>#3",
                "<EMAIL_ADDRESS>",
                "<EMAIL_ADDRESS>#2",
                "<EMAIL_ADDRESS>#4"
            ]
        );
    }

    #[test]
    fn only_the_detected_span_of_a_key_is_rewritten() {
        let doc = json!({"user:alice@example.com": 1});
        assert_eq!(
            redact_doc(&under(privacy::Operator::Replace), doc.clone()),
            json!({"user:<EMAIL_ADDRESS>": 1})
        );
        assert_eq!(redact_doc(&under(privacy::Operator::Redact), doc), json!({"user:": 1}));
    }

    #[test]
    fn keys_without_spans_are_untouched() {
        let r = Redactor::with_defaults();
        let doc = json!({"b": {"c": 1}, "a": [{"z": 2, "y": 3}]});
        let out = redact_doc(&r, doc.clone());
        assert_eq!(out, doc);
        assert_eq!(keys(&out), ["b", "a"]);
        assert_eq!(keys(&out["a"][0]), ["z", "y"]);
    }

    #[test]
    fn keys_inside_arrays_are_renamed() {
        let out = redact_doc(&Redactor::with_defaults(), json!([{"a@b.com": 1}]));
        assert_eq!(out, json!([{"<EMAIL_ADDRESS>": 1}]));
    }

    #[test]
    fn key_renaming_is_deterministic() {
        let r = under(privacy::Operator::Hash);
        let doc = json!({"a@b.com": 1, "<EMAIL_ADDRESS_x>": 2, "a@b.com ": 3, "c@d.com": {"e@f.com": 4}});
        let first = redact_doc(&r, doc.clone());
        let second = redact_doc(&r, doc);
        assert_eq!(
            serde_json::to_string(&first).expect("serializes"),
            serde_json::to_string(&second).expect("serializes")
        );
    }

    #[test]
    fn a_key_is_analyzed_with_its_parent_key_path() {
        let r = Redactor::with_analyzer(weak_phone_analyzer()).with_analyze_options(AnalyzeOptions {
            min_score: Score::from_static(0.4),
            context: Some(ctx_settings()),
        });
        let mut rows = vec![json!({"phone": {"415 555 0142": 1}, "name": {"415 555 0142": 1}})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(
            rows[0],
            json!({"phone": {"<PHONE_NUMBER>": 1}, "name": {"415 555 0142": 1}})
        );
        assert_eq!(stats.keys_rewritten, 1);
    }

    #[test]
    fn children_of_a_renamed_key_keep_its_original_words_as_context() {
        // The phone recognizer is boosted only by `secretword`, which is itself
        // detected (as a person), so the key is renamed to `<PERSON>`.
        let digits = Pattern::new("digits", r"\b\d{3} \d{3} \d{4}\b", Score::from_static(0.1)).expect("static");
        let phone = Recognizer::new(Entity::PhoneNumber, vec![digits])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["secretword"]);
        let word = Pattern::new("word", r"\bsecretword\b", Score::from_static(0.9)).expect("static");
        let mut analyzer = Analyzer::empty();
        analyzer.register(phone);
        analyzer.register(Recognizer::new(Entity::Person, vec![word]).expect("non-empty"));
        let r = Redactor::with_analyzer(analyzer).with_analyze_options(AnalyzeOptions {
            min_score: Score::from_static(0.4),
            context: Some(ctx_settings()),
        });
        let mut rows = vec![json!({"doc": {"secretword": {"x": "415 555 0142"}}})];
        r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"doc": {"<PERSON>": {"x": "<PHONE_NUMBER>"}}}));
    }

    #[test]
    fn row_level_keys_are_never_renamed() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"jane.doe@example.com": 1, "a@b.com": "x"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0], json!({"jane.doe@example.com": 1, "a@b.com": "x"}));
        assert_eq!(stats.total, 0);
        assert_eq!(stats.keys_rewritten, 0);
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
        let r = Redactor::new(Some(Analyzer::with_defaults()), privacy::Operator::Redact, TEST_KEY);
        let mut rows = vec![json!({"v": "x@y.com"})];
        let _ = r.apply(&mut rows).expect("apply ok");
        // Whole-leaf match under `redact` → "" (Value::String, key preserved).
        assert_eq!(rows[0]["v"], "");
        assert!(rows[0]["v"].is_string());
        assert!(rows[0].get("v").is_some());
    }

    #[test]
    fn whole_leaf_match_mask_matches_text_column() {
        let r = Redactor::new(Some(Analyzer::with_defaults()), privacy::Operator::Mask, TEST_KEY);
        let mut json_rows = vec![json!({"v": "x@y.com"})];
        let mut text_rows = vec![Value::String("x@y.com".to_owned())];
        let _ = r.apply(&mut json_rows).expect("apply ok");
        let _ = r.apply(&mut text_rows).expect("apply ok");
        assert_eq!(json_rows[0]["v"], text_rows[0]);
    }

    #[test]
    fn whole_leaf_match_hash_matches_text_column() {
        let r = Redactor::new(Some(Analyzer::with_defaults()), privacy::Operator::Hash, KEY.as_bytes());
        let mut json_rows = vec![json!({"v": "x@y.com"})];
        let mut text_rows = vec![Value::String("x@y.com".to_owned())];
        let _ = r.apply(&mut json_rows).expect("apply ok");
        let _ = r.apply(&mut text_rows).expect("apply ok");
        assert_eq!(json_rows[0]["v"], text_rows[0]);
        assert_eq!(text_rows[0], "<EMAIL_ADDRESS_06fff299b93c0784>");
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
        assert!(
            matches!(&err, RedactionError::Internal(msg) if msg.contains("panicked")),
            "{err}"
        );
    }

    #[test]
    fn panic_at_depth_propagates_internal_error() {
        let mut analyzer = Analyzer::empty();
        analyzer.register(panicking_rule());
        let r = Redactor::with_analyzer(analyzer);
        // PII-bearing string lives 4 levels deep.
        let mut rows = vec![json!({"a": {"b": {"c": {"d": "anything"}}}})];
        let err = r.apply(&mut rows).expect_err("must fail-closed at any depth");
        assert!(
            matches!(&err, RedactionError::Internal(msg) if msg.contains("panicked")),
            "{err}"
        );
    }

    #[test]
    fn panic_while_renaming_a_key_fails_closed() {
        let mut analyzer = Analyzer::empty();
        analyzer.register(panicking_rule());
        let r = Redactor::with_analyzer(analyzer);
        // No string leaf: only the key reaches the panicking recognizer.
        let mut rows = vec![json!({"doc": {"anything": 1}})];
        let err = r.apply(&mut rows).expect_err("must fail-closed");
        assert!(
            matches!(&err, RedactionError::Internal(msg) if msg.contains("panicked")),
            "{err}"
        );
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
        let r = Redactor::with_analyzer(weak_phone_analyzer()).with_analyze_options(AnalyzeOptions {
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
        let r = Redactor::with_analyzer(weak_phone_analyzer()).with_analyze_options(AnalyzeOptions {
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
        let r = Redactor::with_analyzer(weak_phone_analyzer()).with_analyze_options(AnalyzeOptions {
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
        let r_off = Redactor::with_analyzer(weak_phone_analyzer());
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
    fn zip_code_column_redacts_postcode_via_zip_keyword() {
        let r = Redactor::with_defaults();
        let mut rows = vec![json!({"zip_code": "41100"})];
        let stats = r.apply(&mut rows).expect("apply ok");
        assert_eq!(rows[0]["zip_code"], "<POSTCODE>");
        assert_eq!(stats.by_entity.get(&Entity::Postcode).copied(), Some(1));
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
        assert!(Redactor::from_config(None).expect("ok").is_none());
    }

    const TEXT: &str = "abcdefghijklmnopqrstuvwxyz";

    fn rr(entity: Entity, start: usize, end: usize, score: f32, detector: Detector) -> RecognizerResult {
        use crate::result::AnalysisExplanation;
        use crate::validation::ValidationOutcome;
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
                requires_context: false,
            },
            detector,
        }
    }

    /// Runs [`redact_spans`] over a copy of `text`, returning the text and stats.
    fn run_redact_spans(
        text: &str,
        regex: Vec<RecognizerResult>,
        ner: Vec<RecognizerResult>,
        min_score: f32,
        operator: &Operator,
    ) -> (String, RedactionStats) {
        let mut s = text.to_owned();
        let mut stats = RedactionStats::default();
        redact_spans(&mut s, regex, ner, Score::from_static(min_score), operator, &mut stats);
        (s, stats)
    }

    #[test]
    fn redact_spans_drops_ner_below_min_score() {
        let ner = vec![rr(Entity::Person, 0, 4, 0.3, Detector::Ner)];
        let (out, stats) = run_redact_spans(TEXT, Vec::new(), ner, 0.5, &Operator::Replace);
        assert_eq!(out, TEXT, "sub-threshold NER span must be dropped");
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn redact_spans_keeps_disjoint_regex_and_ner() {
        let regex = vec![rr(Entity::EmailAddress, 10, 25, 1.0, Detector::Pattern)];
        let ner = vec![rr(Entity::Person, 0, 5, 0.9, Detector::Ner)];
        let (out, stats) = run_redact_spans(TEXT, regex, ner, 0.4, &Operator::Replace);
        assert_eq!(out, "<PERSON>fghij<EMAIL_ADDRESS>z");
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_entity.get(&Entity::Person), Some(&1));
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress), Some(&1));
    }

    #[test]
    fn redact_spans_container_wins_over_contained_ner() {
        // A regex e-mail span contains an NER person guess over its local part.
        let regex = vec![rr(Entity::EmailAddress, 0, 16, 1.0, Detector::Pattern)];
        let ner = vec![rr(Entity::Person, 0, 10, 0.6, Detector::Ner)];
        let (out, stats) = run_redact_spans(TEXT, regex, ner, 0.4, &Operator::Replace);
        assert_eq!(out, "<EMAIL_ADDRESS>qrstuvwxyz");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.by_entity.get(&Entity::Person), None);
    }

    /// Runs the default regex analyzer, adds one synthetic NER span over `ner`, and redacts.
    fn redact_with_ner(text: &str, ner: &str, entity: Entity, operator: &Operator) -> String {
        let start = text.find(ner).expect("ner text present");
        let regex = Analyzer::with_defaults().analyze(text, &AnalyzeOptions::default());
        let ner = vec![rr(entity, start, start + ner.len(), 0.99, Detector::Ner)];
        run_redact_spans(text, regex, ner, 0.4, operator).0
    }

    #[test]
    fn ner_never_relabels_a_trimmed_pattern_span() {
        // The IBAN span strictly contains the NER span, so the IBAN keeps the
        // bytes the card leaves; resolving the regex spans first would trim the
        // IBAN to 6..12, inside the NER span, and hand those bytes to PERSON.
        // Card 0..6 (0.9) and IBAN 4..12 (0.5) overlap on `aaaabbccdddd`.
        let mut analyzer = Analyzer::empty();
        let card = Pattern::new("a", r"a+b+", Score::from_static(0.9)).expect("static");
        let iban = Pattern::new("b", r"b+c+d+", Score::from_static(0.5)).expect("static");
        analyzer.register(Recognizer::new(Entity::CreditCard, vec![card]).expect("non-empty"));
        analyzer.register(Recognizer::new(Entity::IbanCode, vec![iban]).expect("non-empty"));
        let text = "aaaabbccdddd";
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: None,
        };
        let regex = analyzer.analyze_with_context(text, &[], &opts);
        let ner = vec![rr(Entity::Person, 5, 12, 0.99, Detector::Ner)];
        let (out, stats) = run_redact_spans(text, regex, ner, 0.0, &Operator::Replace);
        assert_eq!(out, "<CREDIT_CARD><IBAN_CODE>");
        assert_eq!(stats.by_entity.get(&Entity::Person), None);
    }

    #[test]
    fn ner_person_inside_email_redacts_whole_address() {
        let out = redact_with_ner(
            "Contact: anna.schmidt@company.example",
            "anna.schmidt",
            Entity::Person,
            &Operator::Replace,
        );
        assert_eq!(out, "Contact: <EMAIL_ADDRESS>");
    }

    #[test]
    fn ner_person_inside_quoted_or_bracketed_email_keeps_delimiters() {
        let quoted = redact_with_ner("\"anna@company.example\"", "anna", Entity::Person, &Operator::Replace);
        assert_eq!(quoted, "\"<EMAIL_ADDRESS>\"");
        let bracketed = redact_with_ner("<anna@company.example>", "anna", Entity::Person, &Operator::Replace);
        assert_eq!(bracketed, "<<EMAIL_ADDRESS>>");
    }

    #[test]
    fn ner_span_inside_iban_redacts_whole_iban() {
        // The IBAN pattern matches the compact form only (spaced groups are not an IBAN).
        for entity in [Entity::Person, Entity::Location] {
            let out = redact_with_ner("IBAN DE89370400440532013000", "3704", entity, &Operator::Replace);
            assert_eq!(out, "IBAN <IBAN_CODE>", "{entity:?}");
        }
    }

    #[test]
    fn ner_person_inside_url_path_redacts_whole_url() {
        let out = redact_with_ner(
            "https://example.com/users/anna",
            "anna",
            Entity::Person,
            &Operator::Replace,
        );
        assert_eq!(out, "<URL>");
    }

    #[test]
    fn ner_person_running_into_email_is_trimmed() {
        let out = redact_with_ner(
            "Anna Schmidt anna@company.example",
            "Anna Schmidt anna",
            Entity::Person,
            &Operator::Replace,
        );
        assert_eq!(out, "<PERSON> <EMAIL_ADDRESS>");
    }

    #[test]
    fn trimmed_overlap_keeps_email_hash_token_stable() {
        let operator = Operator::hash(b"0123456789abcdef0123456789abcdef");
        let address = "anna@company.example";
        let regex = Analyzer::with_defaults().analyze(address, &AnalyzeOptions::default());
        let alone = crate::anonymize(address, regex, &operator).text;
        let out = redact_with_ner(
            "Anna Schmidt anna@company.example",
            "Anna Schmidt anna",
            Entity::Person,
            &operator,
        );
        let (person, email) = out.split_once(' ').expect("two tokens");
        assert!(person.starts_with("<PERSON_"), "{out}");
        assert_eq!(email, alone);
    }

    #[test]
    fn from_config_bad_model_path_fails_closed() {
        let cfg = privacy::Config {
            model: Some(std::path::PathBuf::from("/nonexistent/model/dir")),
            ..privacy::Config::default()
        };
        let err = Redactor::from_config(Some(&cfg)).expect_err("unreadable model must fail closed");
        assert!(matches!(err, RedactorInitError::Ner(_)));
    }

    #[test]
    fn full_without_a_model_runs_no_ner() {
        assert!(!build(&privacy::Config::default()).uses_ner());
    }

    /// A `rules`-mode config with no rules.
    fn rules_config() -> privacy::Config {
        privacy::Config {
            mode: privacy::Mode::Rules,
            ..privacy::Config::default()
        }
    }

    #[test]
    fn rules_mode_never_loads_a_model() {
        let cfg = privacy::Config {
            model: Some(std::path::PathBuf::from("/nonexistent/model/dir")),
            ..rules_config()
        };
        assert!(!build(&cfg).uses_ner());
    }

    #[test]
    fn rules_mode_leaves_error_text_unchanged() {
        let message = "Duplicate entry 'jane@doe.com' for key 'email'";
        let redactor = Some(build(&rules_config()));
        assert_eq!(redactor.redact_message(message), message);
    }

    #[tokio::test]
    async fn rules_mode_leaves_rows_unchanged() {
        let redactor = Some(build(&rules_config()));
        let out = redactor.redact_rows(vec![email_row()]).await.expect("passes through");
        assert_eq!(out, [email_row()]);
    }

    #[test]
    fn ner_allowance_unset_categories_allows_all() {
        let allowed = allowed_ner_entities(&privacy::Config::default());
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
        let cfg = privacy::Config {
            categories: Some(vec![privacy::Category::Personal]),
            ..privacy::Config::default()
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
        let cfg = privacy::Config {
            categories: Some(vec![privacy::Category::Contact]),
            ..privacy::Config::default()
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
        let cfg = privacy::Config {
            categories: Some(vec![privacy::Category::Financial]),
            ..privacy::Config::default()
        };
        assert!(
            allowed_ner_entities(&cfg).is_empty(),
            "a non-NER category must allow no NER entities (model not loaded)"
        );
    }

    #[test]
    fn ner_redactor_preserves_non_pii_leaves() {
        // Exercises the batched-NER deferral in `apply`: a leaf whose text is
        // moved out for inference must be restored verbatim when it yields no
        // spans. Dropping the restore would silently blank every non-PII string
        // under NER, so this guards that invariant. Skips without a local model.
        let Some(dir) = std::env::var_os("PRIVACY_TEST_MODEL").map(std::path::PathBuf::from) else {
            eprintln!("PRIVACY_TEST_MODEL unset; skipping NER redactor test");
            return;
        };
        let cfg = privacy::Config {
            model: Some(dir),
            ..privacy::Config::default()
        };
        let r = Redactor::from_config(Some(&cfg))
            .expect("config ok")
            .expect("redactor present");
        let mut rows = vec![json!({
            "name": "Alice Johnson",
            "clean": "status code 200 returned",
            "n": 42,
        })];
        r.apply(&mut rows).expect("apply ok");
        // Non-PII leaf survives verbatim (the moved-out text was restored).
        assert_eq!(rows[0]["clean"], "status code 200 returned");
        assert_eq!(rows[0]["n"], 42);
        // The person name is redacted (changed from the original).
        assert_ne!(rows[0]["name"], "Alice Johnson");
    }

    #[tokio::test]
    async fn maybe_redact_none_passes_rows_through_unchanged() {
        let redactor: Option<Redactor> = None;
        let rows = vec![email_row()];
        let out = redactor.redact_rows(rows.clone()).await.expect("none passes through");
        assert_eq!(out, rows);
    }

    #[tokio::test]
    async fn maybe_redact_some_redacts_like_the_inner_redactor() {
        let redactor = Some(Redactor::with_defaults());
        let out = redactor.redact_rows(vec![email_row()]).await.expect("some redacts");
        assert_eq!(out[0]["msg"], "ping me at <EMAIL_ADDRESS>");
    }

    #[test]
    fn redact_text_redacts_free_text() {
        let text = "Duplicate entry 'jane@doe.com' for key 'email'";
        let out = Redactor::with_defaults().redact_text(text).expect("redacts");
        assert_eq!(out, "Duplicate entry '<EMAIL_ADDRESS>' for key 'email'");
    }

    #[test]
    fn maybe_redact_message_redacts_or_passes_through() {
        let message = "no such table: jane@doe.com";
        let none: Option<Redactor> = None;
        assert_eq!(none.redact_message(message), message);
        let some = Some(Redactor::with_defaults());
        assert_eq!(some.redact_message(message), "no such table: <EMAIL_ADDRESS>");
    }

    #[test]
    fn from_config_maps_each_operator() {
        let cfg = |operator| privacy::Config {
            operator,
            ..privacy::Config::default()
        };
        let global = |operator| {
            let r = build(&cfg(operator));
            r.operator(r.global)
        };
        assert!(matches!(global(privacy::Operator::Replace), Operator::Replace));
        assert!(matches!(global(privacy::Operator::Mask), Operator::Mask { .. }));
        assert!(matches!(global(privacy::Operator::Redact), Operator::Redact));
        assert!(matches!(global(privacy::Operator::Hash), Operator::Hash { .. }));
    }

    #[test]
    fn from_config_hash_uses_the_configured_key() {
        let r = build(&hash_config(Some(KEY)));
        assert_eq!(
            r.redact_text("jane.doe@example.com").expect("redacts"),
            "<EMAIL_ADDRESS_4e1f0dd69c51e84c>"
        );
    }

    #[test]
    fn from_config_without_key_draws_a_random_key_per_redactor() {
        let cfg = hash_config(None);
        let (a, b) = (build(&cfg), build(&cfg));
        let token = a.redact_text("x@y.com").expect("redacts");
        assert!(token.starts_with("<EMAIL_ADDRESS_") && token.len() == "<EMAIL_ADDRESS_>".len() + 16);
        assert_eq!(
            a.redact_text("x@y.com").expect("redacts"),
            token,
            "stable within one redactor"
        );
        assert_ne!(b.redact_text("x@y.com").expect("redacts"), token);
    }

    #[test]
    fn hash_token_is_the_same_on_every_surface() {
        let r = build(&hash_config(Some(KEY)));
        let email = "jane.doe@example.com";
        let mut rows = vec![json!(email), json!({"outer": {"inner": [ {"email": email} ]}})];
        r.apply(&mut rows).expect("apply ok");
        let cell = rows[0].as_str().expect("string");
        assert_eq!(rows[1]["outer"]["inner"][0]["email"], cell);
        let message = r
            .redact_text(&format!("Duplicate entry '{email}' for key 'email'"))
            .expect("redacts");
        assert_eq!(message, format!("Duplicate entry '{cell}' for key 'email'"));
    }

    #[test]
    fn redact_text_without_pii_is_unchanged() {
        let text = "no such table: customers";
        let out = Redactor::with_defaults().redact_text(text).expect("redacts");
        assert_eq!(out, text);
    }
}
