//! Optional ML/NER recognizer pass: pure-Rust BERT token-classification.
//!
//! Loads a user-provided model directory
//! (`tokenizer.json`, `config.json`, `model.safetensors`) and detects person
//! and location spans via `candle` + `tokenizers`. Emitted [`RecognizerResult`]s
//! carry byte offsets, so they merge with the regex recognizers through
//! `crate::overlap::resolve` exactly like any pattern hit.
//!
//! This module owns the decode glue (softmax, BIO decoding, subword
//! aggregation, scoring, label mapping); the engine wiring lives alongside.

use std::borrow::Cow;
use std::path::Path;

use candle_core::{Device, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::bert::{self, BertModel};
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
/// Returns [`NerError::Load`] when the field is absent, has a non-numeric key,
/// a non-string value, or an index outside the map's length.
fn parse_id2label(raw: &str) -> Result<Vec<String>, NerError> {
    let cfg: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| NerError::Load(format!("config.json parse: {e}")))?;
    let map = cfg
        .get("id2label")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| NerError::Load("config.json missing id2label object".to_owned()))?;
    let mut labels = vec![String::new(); map.len()];
    for (key, value) in map {
        let idx: usize = key
            .parse()
            .map_err(|_| NerError::Load(format!("non-numeric id2label key: {key}")))?;
        let label = value
            .as_str()
            .ok_or_else(|| NerError::Load(format!("id2label[{key}] is not a string")))?;
        let slot = labels
            .get_mut(idx)
            .ok_or_else(|| NerError::Load(format!("id2label index {idx} out of range")))?;
        label.clone_into(slot);
    }
    Ok(labels)
}

/// Reports whether any BIO label maps to a person or location entity.
///
/// Shares [`parse_bio`] with decoding, so the load gate accepts exactly the
/// models `run_window` can decode — rejecting one that emits no target label.
fn has_person_or_location(labels: &[String]) -> bool {
    labels.iter().any(|label| parse_bio(label).0.is_some())
}

/// Maps a label core (without the `B-`/`I-` prefix) to a built-in entity.
fn entity_for_core(core: &str) -> Option<Entity> {
    match core {
        "PER" | "PERSON" => Some(Entity::Person),
        "LOC" | "GPE" => Some(Entity::Location),
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

/// Loaded token-classification model plus its tokenizer and label map.
///
/// Cheap to share behind an `Arc`. candle's `forward` takes `&self`, so
/// concurrent requests run inference without a lock.
pub struct NerEngine {
    model: BertModel,
    classifier: Linear,
    device: Device,
    tokenizer: Tokenizer,
    id2label: Vec<String>,
    threshold: Score,
    allow_person: bool,
    allow_location: bool,
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
    /// Loads a model from `model_dir` (`config.json`, `tokenizer.json`, `model.safetensors`).
    ///
    /// `threshold` is the minimum aggregated span confidence to emit.
    ///
    /// # Errors
    ///
    /// Returns [`NerError::Load`] when any artifact is missing or unreadable,
    /// `config.json` lacks a usable `id2label` exposing a person/location
    /// label, or the weights fail to load.
    pub fn load(model_dir: &Path, threshold: Score) -> Result<Self, NerError> {
        let device = Device::Cpu;

        let raw = std::fs::read_to_string(model_dir.join("config.json"))
            .map_err(|e| NerError::Load(format!("config.json: {e}")))?;
        let id2label = parse_id2label(&raw)?;
        if !has_person_or_location(&id2label) {
            return Err(NerError::Load("model exposes no PERSON or LOCATION label".to_owned()));
        }
        let config: bert::Config =
            serde_json::from_str(&raw).map_err(|e| NerError::Load(format!("config.json fields: {e}")))?;

        let mut tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| NerError::Load(format!("tokenizer.json: {e}")))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_SEQ_LEN,
                stride: STRIDE,
                ..Default::default()
            }))
            .map_err(|e| NerError::Load(format!("truncation config: {e}")))?;

        let weights = model_dir.join("model.safetensors");
        // SAFETY: the weights file is mmaped read-only for the engine's lifetime.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], bert::DTYPE, &device) }
            .map_err(|e| NerError::Load(format!("model.safetensors: {e}")))?;

        let model = BertModel::load(vb.clone(), &config).map_err(|e| NerError::Load(format!("bert weights: {e}")))?;
        let classifier = candle_nn::linear(config.hidden_size, id2label.len(), vb.pp("classifier"))
            .map_err(|e| NerError::Load(format!("classifier weights: {e}")))?;

        Ok(Self {
            model,
            classifier,
            device,
            tokenizer,
            id2label,
            threshold,
            allow_person: true,
            allow_location: true,
        })
    }

    /// Restricts which entities the engine emits (respects `--pii-categories`).
    ///
    /// A span whose entity is disallowed is dropped during decoding, so it
    /// neither redacts nor displaces a regex hit.
    pub(crate) fn set_allowed(&mut self, person: bool, location: bool) {
        self.allow_person = person;
        self.allow_location = location;
    }

    /// Reports whether a decoded entity is permitted by the category filter.
    fn entity_allowed(&self, entity: Option<Entity>) -> bool {
        match entity {
            Some(Entity::Person) => self.allow_person,
            Some(Entity::Location) => self.allow_location,
            _ => true,
        }
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
        if seq == 0 {
            return Ok(());
        }

        let ids_t = Tensor::new(ids, &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| NerError::Inference(format!("input_ids: {e}")))?;
        let mask_t = Tensor::new(enc.get_attention_mask(), &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| NerError::Inference(format!("attention_mask: {e}")))?;
        let types_t = Tensor::new(enc.get_type_ids(), &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| NerError::Inference(format!("token_type_ids: {e}")))?;

        let sequence = self
            .model
            .forward(&ids_t, &types_t, Some(&mask_t))
            .map_err(|e| NerError::Inference(format!("forward: {e}")))?;
        let logits = self
            .classifier
            .forward(&sequence)
            .map_err(|e| NerError::Inference(format!("classifier: {e}")))?;
        let per_token = logits
            .to_vec3::<f32>()
            .map_err(|e| NerError::Inference(format!("extract logits: {e}")))?;
        let Some(token_logits) = per_token.first() else {
            return Ok(());
        };

        let offsets = enc.get_offsets();
        let special = enc.get_special_tokens_mask();
        let mut tags = Vec::with_capacity(seq);
        for t in 0..seq {
            if special.get(t).copied() == Some(1) {
                tags.push(TokenTag::outside());
                continue;
            }
            let logit_row = token_logits.get(t).map_or(&[][..], Vec::as_slice);
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

        results.extend(decode_spans(&tags, self.threshold).into_iter().map(span_to_result));
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
    fn parse_bio_ignores_outside_and_unmapped() {
        assert_eq!(parse_bio("O"), (None, false));
        assert_eq!(parse_bio("B-ORG"), (None, true));
        assert_eq!(parse_bio("B-MISC"), (None, true));
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
    fn has_person_or_location_detects_targets() {
        assert!(has_person_or_location(&[
            "O".to_owned(),
            "B-PER".to_owned(),
            "I-LOC".to_owned()
        ]));
    }

    #[test]
    fn has_person_or_location_false_without_targets() {
        assert!(!has_person_or_location(&[
            "O".to_owned(),
            "B-ORG".to_owned(),
            "B-MISC".to_owned()
        ]));
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
