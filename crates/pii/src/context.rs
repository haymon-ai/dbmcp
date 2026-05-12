//! Context-aware confidence boost engine.
//!
//! Lifts a [`crate::RecognizerResult`] score when one of its source
//! recognizer's context keywords sits in the prefix / suffix word window
//! around the match, or in an external context list supplied by the
//! integration layer (e.g. the redactor's JSON key path split into
//! lowercase words). Word boundaries come from [`crate::words`]'s
//! `char::is_alphanumeric` scan (Unicode `\w+`-style); no lemmatisation
//! step.

use std::borrow::Cow;

use crate::recognizers::Recognizer;
use crate::result::RecognizerResult;
use crate::score::{MAX_SCORE, Score};
use crate::words::{first_word, words};

/// Matching mode for context keyword comparison.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ContextMatchingMode {
    /// Window word must equal the keyword (case-insensitive).
    WholeWord,
    /// Window word must contain the keyword as a substring (case-insensitive).
    #[default]
    Substring,
}

/// Per-call tuning for the boost pass.
///
/// Constructed via [`Default::default`] — values are not user-tunable today.
/// `similarity_factor=0.35`, `min_score_with_context=0.4`, `prefix_words=5`,
/// `suffix_words=0`, `matching_mode=Substring`.
#[derive(Debug, Clone)]
pub struct ContextSettings {
    /// Score increment applied on a successful keyword match.
    pub similarity_factor: Score,
    /// Score floor after a boost is applied.
    pub min_score_with_context: Score,
    /// Number of words before the match included in the window.
    pub prefix_words: u16,
    /// Number of words after the match included in the window.
    pub suffix_words: u16,
    /// Whole-word vs substring keyword comparison.
    pub matching_mode: ContextMatchingMode,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            similarity_factor: Score::from_static(0.35),
            min_score_with_context: Score::from_static(0.4),
            prefix_words: 5,
            suffix_words: 0,
            matching_mode: ContextMatchingMode::Substring,
        }
    }
}

/// Apply the context boost pass to `results` in place, returning the
/// (possibly mutated) vector.
///
/// All `results` MUST originate from `recognizer` — the caller pairs each
/// per-recognizer batch with its producer. `external_context` is a slice
/// of already-lowercased word tokens supplied by the integration layer
/// (e.g. the redactor's JSON key path).
#[must_use]
pub(crate) fn apply_context_boost(
    text: &str,
    mut results: Vec<RecognizerResult>,
    recognizer: &Recognizer,
    external_context: &[String],
    settings: &ContextSettings,
) -> Vec<RecognizerResult> {
    let keywords = recognizer.context();
    if keywords.is_empty() && external_context.is_empty() {
        return results;
    }
    for result in &mut results {
        boost_one(text, result, keywords, external_context, settings);
    }
    results
}

fn boost_one(
    text: &str,
    result: &mut RecognizerResult,
    keywords: &[&'static str],
    external_context: &[String],
    settings: &ContextSettings,
) {
    if result.score == MAX_SCORE {
        return;
    }

    let window = collect_window(
        text,
        result.start,
        result.end,
        settings.prefix_words,
        settings.suffix_words,
    );

    let Some(hit) = find_supportive_keyword(&window, external_context, keywords, settings.matching_mode) else {
        return;
    };

    let boosted = boost_score(
        result.score,
        settings.similarity_factor,
        settings.min_score_with_context,
    );
    result.score = boosted;
    result.explanation.final_score = boosted;
    result.explanation.supportive_keyword = Some(Cow::Borrowed(hit));
}

fn collect_window(text: &str, start: usize, end: usize, prefix: u16, suffix: u16) -> Vec<String> {
    let safe_start = safe_left_boundary(text, start);
    let safe_end = safe_right_boundary(text, end);
    let mut out = Vec::new();

    // Prefix: walk backward from `start` taking up to `prefix + 1` words.
    // The extra slot accommodates concatenated forms (FR-019).
    let mut prefix_words = words(&text[..safe_start]);
    let take = usize::from(prefix).saturating_add(1);
    let drop_n = prefix_words.len().saturating_sub(take);
    prefix_words.drain(..drop_n);
    out.extend(prefix_words);

    // Slack-slot reaches into the leading characters of the match itself
    // (covers concatenated forms like "card4012..."): include the first
    // word of the match's own text.
    if let Some(w) = first_word(&text[safe_start..safe_end]) {
        out.push(w);
    }

    // Suffix: take up to `suffix` words from the tail.
    if suffix > 0 {
        out.extend(words(&text[safe_end..]).into_iter().take(usize::from(suffix)));
    }

    out
}

fn safe_left_boundary(text: &str, start: usize) -> usize {
    let mut s = start.min(text.len());
    while s > 0 && !text.is_char_boundary(s) {
        s -= 1;
    }
    s
}

fn safe_right_boundary(text: &str, end: usize) -> usize {
    let mut e = end.min(text.len());
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    e
}

fn find_supportive_keyword(
    window: &[String],
    external_context: &[String],
    keywords: &[&'static str],
    mode: ContextMatchingMode,
) -> Option<&'static str> {
    // Window words and external_context are already lowercased; recognizer
    // keywords are pre-lowercased (debug_asserted by `with_context`).
    keywords.iter().copied().find(|&kw| {
        window.iter().chain(external_context.iter()).any(|w| match mode {
            ContextMatchingMode::WholeWord => w == kw,
            ContextMatchingMode::Substring => w.contains(kw),
        })
    })
}

fn boost_score(current: Score, factor: Score, floor: Score) -> Score {
    // current, factor, floor are all in [0, 1] → sum is non-negative; clamp the top.
    let raw = current.as_f32() + factor.as_f32();
    let clamped = raw.max(floor.as_f32()).min(MAX_SCORE.as_f32());
    Score::new(clamped).expect("clamp guarantees [0, 1]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entity;
    use crate::pattern::Pattern;
    use crate::result::AnalysisExplanation;
    use crate::validation::ValidationOutcome;

    fn default_settings() -> ContextSettings {
        ContextSettings {
            similarity_factor: Score::from_static(0.35),
            min_score_with_context: Score::from_static(0.4),
            prefix_words: 5,
            suffix_words: 0,
            matching_mode: ContextMatchingMode::WholeWord,
        }
    }

    fn dummy_recognizer(name: &'static str, ctx: &'static [&'static str]) -> Recognizer {
        let pat = Pattern::new("p", r"\d+", Score::from_static(0.3)).expect("static");
        Recognizer::new(Entity::PhoneNumber, vec![pat])
            .expect("non-empty patterns")
            .with_name(name)
            .with_context(ctx)
    }

    fn result(score: f32, name: &'static str, start: usize, end: usize) -> RecognizerResult {
        let s = Score::new(score).expect("valid");
        RecognizerResult {
            entity_type: Entity::PhoneNumber,
            start,
            end,
            score: s,
            explanation: AnalysisExplanation {
                recognizer_name: Cow::Borrowed(name),
                pattern_name: Some(Cow::Borrowed("p")),
                original_score: s,
                validation: ValidationOutcome::Unknown,
                final_score: s,
                supportive_keyword: None,
            },
        }
    }

    #[test]
    fn defaults_match_documented_values() {
        let settings = ContextSettings::default();
        assert_eq!(settings.prefix_words, 5);
        assert_eq!(settings.suffix_words, 0);
        assert!(matches!(settings.matching_mode, ContextMatchingMode::Substring));
    }

    #[test]
    fn boost_skips_already_max_score() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "my phone 415";
        let mut r = result(1.0, "R", 9, 12);
        let out = apply_context_boost(text, vec![r.clone()], &rec, &[], &default_settings());
        assert_eq!(out[0].score, MAX_SCORE);
        assert!(out[0].explanation.supportive_keyword.is_none());
        let _ = &mut r;
    }

    #[test]
    fn boost_skips_when_no_context_and_no_external() {
        let rec = dummy_recognizer("R", &[]);
        let text = "my phone 415";
        let out = apply_context_boost(text, vec![result(0.3, "R", 9, 12)], &rec, &[], &default_settings());
        assert!(out[0].explanation.supportive_keyword.is_none());
    }

    #[test]
    fn boost_lifts_score_above_floor() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "my phone 415";
        let out = apply_context_boost(text, vec![result(0.1, "R", 9, 12)], &rec, &[], &default_settings());
        // 0.1 + 0.35 = 0.45, floor 0.4 → 0.45.
        assert!(out[0].score.as_f32() >= 0.4);
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn boost_clamps_to_max() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "phone here 415";
        let out = apply_context_boost(text, vec![result(0.8, "R", 11, 14)], &rec, &[], &default_settings());
        assert_eq!(out[0].score, MAX_SCORE);
    }

    #[test]
    fn boost_uses_first_matching_keyword_only() {
        let rec = dummy_recognizer("R", &["phone", "telephone"]);
        let text = "phone telephone 415";
        let out = apply_context_boost(text, vec![result(0.1, "R", 16, 19)], &rec, &[], &default_settings());
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn whole_word_excludes_substring() {
        let rec = dummy_recognizer("R", &["card"]);
        let text = "discard 4012";
        let settings = ContextSettings {
            matching_mode: ContextMatchingMode::WholeWord,
            ..default_settings()
        };
        let out = apply_context_boost(text, vec![result(0.1, "R", 8, 12)], &rec, &[], &settings);
        assert!(out[0].explanation.supportive_keyword.is_none());
    }

    #[test]
    fn substring_includes_substring() {
        let rec = dummy_recognizer("R", &["card"]);
        let text = "discard 4012";
        let settings = ContextSettings {
            matching_mode: ContextMatchingMode::Substring,
            ..default_settings()
        };
        let out = apply_context_boost(text, vec![result(0.1, "R", 8, 12)], &rec, &[], &settings);
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("card"));
    }

    #[test]
    fn window_respects_utf8_boundaries() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "naïve café phone 415"; // multibyte chars
        let phone_start = text.find("415").expect("found");
        let out = apply_context_boost(
            text,
            vec![result(0.1, "R", phone_start, phone_start + 3)],
            &rec,
            &[],
            &default_settings(),
        );
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn window_handles_match_at_input_start() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "415 555 0142";
        let out = apply_context_boost(text, vec![result(0.1, "R", 0, 3)], &rec, &[], &default_settings());
        // No preceding context → no boost.
        assert!(out[0].explanation.supportive_keyword.is_none());
    }

    #[test]
    fn concatenated_form_resolved_via_slack_slot() {
        let rec = dummy_recognizer("R", &["card"]);
        // "card4012" — match starts at digit boundary; slack slot covers
        // the leading word of the match's own text.
        let text = "card4012";
        let out = apply_context_boost(text, vec![result(0.1, "R", 4, 8)], &rec, &[], &default_settings());
        // Slack slot picks up the leading "card" via prefix word split.
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("card"));
    }

    #[test]
    fn external_context_alone_can_trigger_boost() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "415"; // no surrounding text
        let external = [String::from("phone")];
        let out = apply_context_boost(text, vec![result(0.1, "R", 0, 3)], &rec, &external, &default_settings());
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn suffix_window_extends_after_match() {
        let rec = dummy_recognizer("R", &["phone"]);
        let text = "415 phone";
        let settings = ContextSettings {
            suffix_words: 2,
            prefix_words: 0,
            ..default_settings()
        };
        let out = apply_context_boost(text, vec![result(0.1, "R", 0, 3)], &rec, &[], &settings);
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn score_never_decreases() {
        let rec = dummy_recognizer("R", &["zzz"]); // never matches
        let text = "no keywords here 415";
        let out = apply_context_boost(text, vec![result(0.3, "R", 17, 20)], &rec, &[], &default_settings());
        assert_eq!(out[0].score, Score::from_static(0.3));
        assert!(out[0].explanation.supportive_keyword.is_none());
    }
}
