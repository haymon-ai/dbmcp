//! Optional ML/NER recognizer pass: ONNX BERT token-classification.
//!
//! Loads a user-provided model directory
//! (`tokenizer.json`, `config.json`, `model.onnx`) and detects person and
//! location spans via `ort` + `tokenizers`. Emitted [`RecognizerResult`]s
//! carry byte offsets, so they merge with the regex recognizers through
//! `crate::overlap::resolve` exactly like any pattern hit.
//!
//! This module owns the decode glue (softmax, BIO decoding, subword
//! aggregation, scoring, label mapping); the engine wiring lives alongside.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use tokenizers::{Tokenizer, TruncationParams};

use crate::entity::Entity;
use crate::result::{AnalysisExplanation, RecognizerResult};
use crate::score::Score;
use crate::validation::ValidationOutcome;

/// Recognizer name stamped on every NER-produced [`RecognizerResult`].
const RECOGNIZER_NAME: &str = "BertNerRecognizer";

/// Maximum tokens per window, including the `[CLS]`/`[SEP]` special tokens.
const MAX_SEQ_LEN: usize = 512;

/// Token overlap between consecutive windows of an over-long input.
const STRIDE: usize = 128;

/// Errors raised while loading or running the NER engine.
///
/// All variants are returned, never panicked — the release profile uses
/// `panic = "abort"`, so callers rely on `Result` for fail-closed behaviour.
#[derive(Debug, thiserror::Error)]
pub enum NerError {
    /// The model directory, tokenizer, or weights could not be opened.
    #[error("NER model load failed: {0}")]
    Load(String),
    /// A tokenization or model forward-pass step failed.
    #[error("NER inference failed: {0}")]
    Inference(String),
}

/// Parses a Hugging Face `config.json` `id2label` object into an indexed list.
///
/// # Errors
///
/// Returns [`NerError::Load`] when the field is absent, a key is non-numeric, a
/// value is not a string, or the indices are not the contiguous range `0..len`.
fn parse_id2label(raw: &str) -> Result<Vec<String>, NerError> {
    #[derive(serde::Deserialize)]
    struct Config {
        id2label: std::collections::BTreeMap<usize, String>,
    }

    let cfg: Config = serde_json::from_str(raw).map_err(|e| NerError::Load(format!("config.json parse: {e}")))?;
    let mut labels = Vec::with_capacity(cfg.id2label.len());
    for (expected, (idx, label)) in cfg.id2label.into_iter().enumerate() {
        if idx != expected {
            return Err(NerError::Load(format!("id2label index {idx} out of range")));
        }
        labels.push(label);
    }
    Ok(labels)
}

/// Entities the decoder can emit; drives the default allow-set and load gate.
pub(crate) const NER_ENTITIES: &[Entity] = &[
    Entity::Person,
    Entity::Location,
    Entity::Organization,
    Entity::Nrp,
    Entity::Facility,
];

/// Reports whether any BIO label maps to a supported NER entity.
///
/// Shares [`parse_bio`] with decoding, so the load gate accepts exactly the
/// models `run_window` can decode — rejecting one that emits no target label.
fn has_supported_entity(labels: &[String]) -> bool {
    labels.iter().any(|label| parse_bio(label).0.is_some())
}

/// Maps a label core (without the `B-`/`I-` prefix) to a built-in entity.
///
/// Mirrors Presidio's `model_to_presidio_entity_mapping`, except `FAC` maps to
/// a distinct [`Entity::Facility`] rather than being folded into location.
fn entity_for_core(core: &str) -> Option<Entity> {
    match core {
        "PER" | "PERSON" => Some(Entity::Person),
        "LOC" | "GPE" => Some(Entity::Location),
        "ORG" | "ORGANIZATION" => Some(Entity::Organization),
        "NORP" | "NRP" => Some(Entity::Nrp),
        "FAC" => Some(Entity::Facility),
        _ => None,
    }
}

/// Splits a BIO label into its mapped entity and a begin flag.
///
/// `"O"` and unmapped cores yield `(None, false)`.
fn parse_bio(label: &str) -> (Option<Entity>, bool) {
    if let Some(core) = label.strip_prefix("B-") {
        (entity_for_core(core), true)
    } else if let Some(core) = label.strip_prefix("I-") {
        (entity_for_core(core), false)
    } else {
        (None, false)
    }
}

/// Argmax index plus the softmax probability of that index.
///
/// Numerically stable; returns `(0, 0.0)` for an empty slice.
fn softmax_argmax(logits: &[f32]) -> (usize, f32) {
    if logits.is_empty() {
        return (0, 0.0);
    }
    let mut best = 0;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &v) in logits.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best = i;
        }
    }
    let sum: f32 = logits.iter().map(|&v| (v - best_val).exp()).sum();
    let prob = if sum > 0.0 { 1.0 / sum } else { 0.0 };
    (best, prob)
}

/// One token's decoded BIO tag with its byte span and confidence.
#[derive(Debug)]
struct TokenTag {
    entity: Option<Entity>,
    is_begin: bool,
    score: f32,
    start: usize,
    end: usize,
}

impl TokenTag {
    /// A non-entity ("O") tag that closes any open span.
    fn outside() -> Self {
        Self {
            entity: None,
            is_begin: false,
            score: 0.0,
            start: 0,
            end: 0,
        }
    }
}

/// An aggregated entity span over one or more contiguous tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    entity: Entity,
    start: usize,
    end: usize,
    score: Score,
}

/// A span being accumulated across contiguous BIO tags of one entity.
#[derive(Clone, Copy)]
struct OpenSpan {
    entity: Entity,
    start: usize,
    end: usize,
    /// Highest token probability seen so far (max-aggregation, see below).
    max_score: f32,
}

/// Collapses subword tags into one tag per word (HF "first" strategy).
///
/// Tokens sharing a `word_id` fold into a single tag: the label comes from the
/// word's first subword, the score is the max over its subwords, and the span
/// runs from the first subword's start to the last subword's end. Tokens with no
/// word id (special tokens) pass through unchanged, closing any open word.
fn aggregate_words(tags: &[TokenTag], word_ids: &[Option<u32>]) -> Vec<TokenTag> {
    let mut out: Vec<TokenTag> = Vec::with_capacity(tags.len());
    let mut current: Option<u32> = None;
    for (tag, &wid) in tags.iter().zip(word_ids) {
        match wid {
            Some(id) if current == Some(id) => {
                if let Some(last) = out.last_mut() {
                    last.end = tag.end;
                    last.score = last.score.max(tag.score);
                }
            }
            Some(id) => {
                current = Some(id);
                out.push(TokenTag {
                    entity: tag.entity,
                    is_begin: tag.is_begin,
                    score: tag.score,
                    start: tag.start,
                    end: tag.end,
                });
            }
            None => {
                current = None;
                out.push(TokenTag::outside());
            }
        }
    }
    out
}

/// Merges contiguous BIO tags into spans, keeping those at or above `threshold`.
///
/// Subword token scores aggregate with the **maximum** strategy: a span's score
/// is the highest token probability it contains, so one weak subword cannot sink
/// an otherwise confident entity.
fn decode_spans(tags: &[TokenTag], threshold: Score) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut open: Option<OpenSpan> = None;

    for tag in tags {
        match tag.entity {
            Some(entity) => {
                let continues = !tag.is_begin && open.as_ref().is_some_and(|o| o.entity == entity);
                if continues {
                    if let Some(o) = open.as_mut() {
                        o.end = tag.end;
                        o.max_score = o.max_score.max(tag.score);
                    }
                } else {
                    flush(&mut spans, open.take(), threshold);
                    open = Some(OpenSpan {
                        entity,
                        start: tag.start,
                        end: tag.end,
                        max_score: tag.score,
                    });
                }
            }
            None => flush(&mut spans, open.take(), threshold),
        }
    }
    flush(&mut spans, open, threshold);
    spans
}

/// Pushes an open span onto `spans` when it is non-empty and meets `threshold`.
fn flush(spans: &mut Vec<Span>, open: Option<OpenSpan>, threshold: Score) {
    let Some(OpenSpan {
        entity,
        start,
        end,
        max_score,
    }) = open
    else {
        return;
    };
    if end <= start {
        return;
    }
    let Ok(score) = Score::new(max_score) else {
        return;
    };
    if score >= threshold {
        spans.push(Span {
            entity,
            start,
            end,
            score,
        });
    }
}

/// Builds a [`RecognizerResult`] from an aggregated [`Span`].
fn span_to_result(span: Span) -> RecognizerResult {
    RecognizerResult {
        entity_type: span.entity,
        start: span.start,
        end: span.end,
        score: span.score,
        explanation: AnalysisExplanation {
            recognizer_name: Cow::Borrowed(RECOGNIZER_NAME),
            pattern_name: None,
            original_score: span.score,
            validation: ValidationOutcome::Unknown,
            final_score: span.score,
            supportive_keyword: None,
        },
    }
}

/// Loaded token-classification session plus its tokenizer and label map.
///
/// Cheap to share behind an `Arc`. `ort` runs inference through `&mut Session`,
/// so the session sits behind a `Mutex` and concurrent requests serialize on it.
pub struct NerEngine {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    id2label: Vec<String>,
    threshold: Score,
    allowed: Vec<Entity>,
    /// Whether the graph declares a `token_type_ids` input (`DistilBERT` omits it).
    needs_token_type_ids: bool,
}

impl std::fmt::Debug for NerEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NerEngine")
            .field("labels", &self.id2label.len())
            .field("threshold", &self.threshold)
            .finish_non_exhaustive()
    }
}

impl NerEngine {
    /// Loads a model from `model_dir` (`config.json`, `tokenizer.json`, `model.onnx`).
    ///
    /// `threshold` is the minimum aggregated span confidence to emit.
    ///
    /// # Errors
    ///
    /// Returns [`NerError::Load`] when any artifact is missing or unreadable,
    /// `config.json` lacks a usable `id2label` exposing a supported NER
    /// label, or the ONNX session fails to initialise.
    pub fn load(model_dir: &Path, threshold: Score) -> Result<Self, NerError> {
        let raw = std::fs::read_to_string(model_dir.join("config.json"))
            .map_err(|e| NerError::Load(format!("config.json: {e}")))?;
        let id2label = parse_id2label(&raw)?;
        if !has_supported_entity(&id2label) {
            return Err(NerError::Load("model exposes no supported NER label".to_owned()));
        }

        let mut tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| NerError::Load(format!("tokenizer.json: {e}")))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_SEQ_LEN,
                stride: STRIDE,
                ..Default::default()
            }))
            .map_err(|e| NerError::Load(format!("truncation config: {e}")))?;

        let session = Session::builder()
            .map_err(|e| NerError::Load(format!("session builder: {e}")))?
            .commit_from_file(model_dir.join("model.onnx"))
            .map_err(|e| NerError::Load(format!("model.onnx: {e}")))?;
        let needs_token_type_ids = session.inputs.iter().any(|i| i.name == "token_type_ids");

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            id2label,
            threshold,
            allowed: NER_ENTITIES.to_vec(),
            needs_token_type_ids,
        })
    }

    /// Restricts which entities the engine emits (respects `--pii-categories`).
    ///
    /// A span whose entity is not in the set is dropped during decoding, so it
    /// neither redacts nor displaces a regex hit.
    pub(crate) fn set_allowed(&mut self, entities: impl IntoIterator<Item = Entity>) {
        self.allowed = entities.into_iter().collect();
    }

    /// Reports whether a decoded entity is permitted by the category filter.
    ///
    /// Non-entity tags (`None`) are never filtered here.
    fn entity_allowed(&self, entity: Option<Entity>) -> bool {
        entity.is_none_or(|e| self.allowed.contains(&e))
    }

    /// Runs NER over one text, returning its spans as [`RecognizerResult`]s.
    ///
    /// Byte offsets index into `text`. Over-long inputs are split into
    /// overlapping windows whose spans merge via `crate::overlap::resolve`.
    ///
    /// # Errors
    ///
    /// Returns [`NerError::Inference`] on a tokenization or forward-pass
    /// failure. Never panics.
    pub fn analyze(&self, text: &str) -> Result<Vec<RecognizerResult>, NerError> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| NerError::Inference(format!("tokenize: {e}")))?;

        let mut results = Vec::new();
        self.run_window(&encoding, &mut results)?;
        for overflow in encoding.get_overflowing() {
            self.run_window(overflow, &mut results)?;
        }
        Ok(crate::overlap::resolve(results))
    }

    /// Runs one tokenized window through the model and appends decoded spans.
    fn run_window(&self, enc: &tokenizers::Encoding, results: &mut Vec<RecognizerResult>) -> Result<(), NerError> {
        let ids = enc.get_ids();
        let seq = ids.len();
        let num_labels = self.id2label.len();
        if seq == 0 || num_labels == 0 {
            return Ok(());
        }

        let len = i64::try_from(seq).map_err(|_| NerError::Inference("sequence too long".to_owned()))?;
        let shape = [1_i64, len];
        // BERT ONNX exports take int64 inputs; build one `[1, seq]` tensor per field.
        let tensor = |name: &str, data: &[u32]| -> Result<Tensor<i64>, NerError> {
            let row: Vec<i64> = data.iter().map(|&v| i64::from(v)).collect();
            Tensor::from_array((shape, row)).map_err(|e| NerError::Inference(format!("{name}: {e}")))
        };
        let mut inputs = ort::inputs![
            "input_ids" => tensor("input_ids", ids)?,
            "attention_mask" => tensor("attention_mask", enc.get_attention_mask())?,
        ];
        // DistilBERT-style models omit token_type_ids; only send it when declared.
        if self.needs_token_type_ids {
            inputs.push((
                "token_type_ids".into(),
                tensor("token_type_ids", enc.get_type_ids())?.into(),
            ));
        }
        let offsets = enc.get_offsets();
        let word_ids = enc.get_word_ids();

        // Hold the session lock for inference and reading logits only. The guard
        // (and the `logits` slice borrowed from it) drop at the end of this block,
        // releasing the Mutex before the pure-CPU aggregation/decoding below.
        let tags = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| NerError::Inference("session lock poisoned".to_owned()))?;
            let outputs = session
                .run(inputs)
                .map_err(|e| NerError::Inference(format!("run: {e}")))?;
            // First graph output holds the logits; guard rather than index so an
            // output-less model fails the request, never panics (release aborts).
            let Some(output) = outputs.values().next() else {
                return Ok(());
            };
            let (_logits_shape, logits) = output
                .try_extract_tensor::<f32>()
                .map_err(|e| NerError::Inference(format!("extract logits: {e}")))?;

            let mut tags = Vec::with_capacity(seq);
            for t in 0..seq {
                // Special tokens ([CLS]/[SEP]) carry no word id; emit a closing "O".
                // This is the same signal `aggregate_words` folds on, so one lookup
                // gates both the skip here and the word grouping below.
                if word_ids.get(t).copied().flatten().is_none() {
                    tags.push(TokenTag::outside());
                    continue;
                }
                let logit_row = logits.get(t * num_labels..(t + 1) * num_labels).unwrap_or(&[]);
                let (best, prob) = softmax_argmax(logit_row);
                let label = self.id2label.get(best).map_or("O", String::as_str);
                let (mut entity, is_begin) = parse_bio(label);
                if !self.entity_allowed(entity) {
                    entity = None;
                }
                let (start, end) = offsets.get(t).copied().unwrap_or((0, 0));
                tags.push(TokenTag {
                    entity,
                    is_begin,
                    score: prob,
                    start,
                    end,
                });
            }
            tags
        };

        let words = aggregate_words(&tags, word_ids);
        results.extend(decode_spans(&words, self.threshold).into_iter().map(span_to_result));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(entity: Option<Entity>, is_begin: bool, score: f32, start: usize, end: usize) -> TokenTag {
        TokenTag {
            entity,
            is_begin,
            score,
            start,
            end,
        }
    }

    #[test]
    fn parse_bio_maps_person_and_location() {
        assert_eq!(parse_bio("B-PER"), (Some(Entity::Person), true));
        assert_eq!(parse_bio("I-PER"), (Some(Entity::Person), false));
        assert_eq!(parse_bio("B-LOC"), (Some(Entity::Location), true));
        assert_eq!(parse_bio("I-GPE"), (Some(Entity::Location), false));
    }

    #[test]
    fn parse_bio_maps_organization_nrp_facility() {
        assert_eq!(parse_bio("B-ORG"), (Some(Entity::Organization), true));
        assert_eq!(parse_bio("I-ORGANIZATION"), (Some(Entity::Organization), false));
        assert_eq!(parse_bio("B-NORP"), (Some(Entity::Nrp), true));
        assert_eq!(parse_bio("B-NRP"), (Some(Entity::Nrp), true));
        assert_eq!(parse_bio("B-FAC"), (Some(Entity::Facility), true));
    }

    #[test]
    fn parse_bio_ignores_outside_and_unmapped() {
        assert_eq!(parse_bio("O"), (None, false));
        assert_eq!(parse_bio("B-MISC"), (None, true));
        assert_eq!(parse_bio("B-DATE"), (None, true));
        assert_eq!(parse_bio("B-MONEY"), (None, true));
    }

    #[test]
    fn softmax_argmax_picks_max_and_normalises() {
        let (idx, prob) = softmax_argmax(&[0.0, 5.0, 1.0]);
        assert_eq!(idx, 1);
        assert!(prob > 0.9, "dominant logit should give high prob, got {prob}");
    }

    #[test]
    fn softmax_argmax_empty_is_zero() {
        assert_eq!(softmax_argmax(&[]), (0, 0.0));
    }

    #[test]
    fn decode_merges_consecutive_same_entity_with_max_score() {
        let tags = [
            tag(Some(Entity::Person), true, 0.9, 0, 5),
            tag(Some(Entity::Person), false, 0.3, 6, 11),
        ];
        let spans = decode_spans(&tags, Score::from_static(0.5));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity, Entity::Person);
        assert_eq!((spans[0].start, spans[0].end), (0, 11));
        // Max aggregation: the strong first subword (0.9) wins over the weak 0.3.
        assert!((spans[0].score.as_f32() - 0.9).abs() < 1e-6, "expected max 0.9");
    }

    #[test]
    fn decode_max_keeps_name_a_mean_would_drop() {
        // Tokens 0.9 then 0.3: mean = 0.6 < 0.7 (would drop), max = 0.9 >= 0.7 (kept).
        let tags = [
            tag(Some(Entity::Person), true, 0.9, 0, 5),
            tag(Some(Entity::Person), false, 0.3, 6, 11),
        ];
        let spans = decode_spans(&tags, Score::from_static(0.7));
        assert_eq!(spans.len(), 1, "max aggregation must keep this span");
    }

    #[test]
    fn decode_splits_on_new_begin() {
        let tags = [
            tag(Some(Entity::Person), true, 0.9, 0, 3),
            tag(Some(Entity::Person), true, 0.9, 4, 7),
        ];
        let spans = decode_spans(&tags, Score::from_static(0.5));
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn decode_outside_token_closes_span() {
        let tags = [
            tag(Some(Entity::Location), true, 0.9, 0, 6),
            TokenTag::outside(),
            tag(Some(Entity::Location), true, 0.9, 10, 16),
        ];
        let spans = decode_spans(&tags, Score::from_static(0.5));
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].entity, Entity::Location);
    }

    #[test]
    fn decode_drops_below_threshold() {
        let tags = [tag(Some(Entity::Person), true, 0.3, 0, 5)];
        let spans = decode_spans(&tags, Score::from_static(0.5));
        assert!(spans.is_empty());
    }

    #[test]
    fn decode_skips_zero_length_span() {
        let tags = [tag(Some(Entity::Person), true, 0.9, 4, 4)];
        let spans = decode_spans(&tags, Score::from_static(0.5));
        assert!(spans.is_empty());
    }

    #[test]
    fn parse_id2label_builds_indexed_list() {
        let raw = r#"{"id2label": {"0": "O", "1": "B-PER", "2": "I-PER"}}"#;
        let labels = parse_id2label(raw).expect("valid id2label");
        assert_eq!(labels, vec!["O", "B-PER", "I-PER"]);
    }

    #[test]
    fn parse_id2label_rejects_missing_field() {
        assert!(parse_id2label("{}").is_err());
    }

    #[test]
    fn parse_id2label_rejects_out_of_range_index() {
        let raw = r#"{"id2label": {"0": "O", "5": "B-PER"}}"#;
        assert!(parse_id2label(raw).is_err());
    }

    #[test]
    fn has_supported_entity_detects_targets() {
        assert!(has_supported_entity(&[
            "O".to_owned(),
            "B-PER".to_owned(),
            "I-LOC".to_owned()
        ]));
    }

    #[test]
    fn has_supported_entity_accepts_org_only_model() {
        // A CoNLL model lacking PER/LOC but exposing ORG must still load.
        assert!(has_supported_entity(&["O".to_owned(), "B-ORG".to_owned()]));
    }

    #[test]
    fn has_supported_entity_false_without_targets() {
        assert!(!has_supported_entity(&[
            "O".to_owned(),
            "B-MISC".to_owned(),
            "B-DATE".to_owned()
        ]));
    }

    #[test]
    fn decode_emits_organization_nrp_facility() {
        for entity in [Entity::Organization, Entity::Nrp, Entity::Facility] {
            let tags = [tag(Some(entity), true, 0.9, 0, 5)];
            let spans = decode_spans(&tags, Score::from_static(0.5));
            assert_eq!(spans.len(), 1, "{entity} span must decode");
            assert_eq!(spans[0].entity, entity);
        }
    }

    #[test]
    fn ner_entities_match_decoder_mapping() {
        // Every advertised NER entity must be reachable through entity_for_core.
        for &entity in NER_ENTITIES {
            let core = match entity {
                Entity::Person => "PER",
                Entity::Location => "LOC",
                Entity::Organization => "ORG",
                Entity::Nrp => "NORP",
                Entity::Facility => "FAC",
                other => panic!("NER_ENTITIES lists an unmapped entity: {other}"),
            };
            assert_eq!(entity_for_core(core), Some(entity));
        }
    }

    #[test]
    fn aggregate_folds_subwords_keeping_first_label_and_full_span() {
        let tags = [
            tag(Some(Entity::Organization), true, 0.6, 0, 2),
            tag(None, false, 0.9, 2, 3),
        ];
        let words = aggregate_words(&tags, &[Some(0), Some(0)]);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].entity, Some(Entity::Organization));
        assert_eq!((words[0].start, words[0].end), (0, 3), "span covers the whole word");
        assert!((words[0].score - 0.9).abs() < 1e-6, "score lifts to the max subword");
    }

    #[test]
    fn aggregate_first_label_suppresses_later_entity_subword() {
        let tags = [
            tag(None, false, 0.3, 0, 2),
            tag(Some(Entity::Organization), true, 0.95, 2, 4),
        ];
        let words = aggregate_words(&tags, &[Some(0), Some(0)]);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].entity, None);
        assert_eq!((words[0].start, words[0].end), (0, 4));
    }

    #[test]
    fn aggregate_passes_special_tokens_through_as_outside() {
        let tags = [
            TokenTag::outside(),
            tag(Some(Entity::Person), true, 0.9, 0, 4),
            TokenTag::outside(),
        ];
        let words = aggregate_words(&tags, &[None, Some(0), None]);
        assert_eq!(words.len(), 3);
        assert!(words[0].entity.is_none());
        assert_eq!(words[1].entity, Some(Entity::Person));
        assert!(words[2].entity.is_none());
    }

    #[test]
    fn aggregate_keeps_distinct_words_separate_for_bio_merge() {
        let tags = [
            tag(Some(Entity::Location), true, 0.9, 0, 3),
            tag(Some(Entity::Location), false, 0.8, 4, 13),
        ];
        let words = aggregate_words(&tags, &[Some(0), Some(1)]);
        assert_eq!(words.len(), 2, "distinct word ids must not fold together");
        let spans = decode_spans(&words, Score::from_static(0.5));
        assert_eq!(spans.len(), 1, "I- continuation merges the two words");
        assert_eq!((spans[0].start, spans[0].end), (0, 13));
    }

    #[test]
    fn span_to_result_stamps_recognizer_name_and_no_pattern() {
        let result = span_to_result(Span {
            entity: Entity::Person,
            start: 0,
            end: 5,
            score: Score::from_static(0.9),
        });
        assert_eq!(result.entity_type, Entity::Person);
        assert_eq!(result.explanation.recognizer_name, RECOGNIZER_NAME);
        assert!(result.explanation.pattern_name.is_none());
    }
}
