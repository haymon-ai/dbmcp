//! Analyzer engine: registry + entry point + analyze-time options.

use dbmcp_config::{PiiCategory, PiiConfig};

use crate::Category;
use crate::context::{ContextSettings, apply_context_boost};
use crate::error::AnalyzerBuildError;
use crate::overlap;
use crate::recognizers::Recognizer;
use crate::result::RecognizerResult;
use crate::score::Score;

/// Per-call overrides handed to [`Analyzer::analyze`].
///
/// `min_score` defaults to [`crate::MIN_SCORE`] (via [`Score::default`]); set higher to
/// drop low-confidence matches before overlap resolution. `context` is
/// `None` by default — the context-aware boost step does NOT run.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    /// Drop results whose score is below this floor before overlap resolution.
    pub min_score: Score,
    /// Per-call settings for the context-aware boost pass. `None` ⇒ feature disabled.
    pub context: Option<ContextSettings>,
}

/// Registry of recognizers and the public entry point for PII analysis.
#[derive(Debug, Default)]
pub struct Analyzer {
    recognizers: Vec<Recognizer>,
    /// Optional ML/NER engine, attached by the redactor when enabled.
    ///
    /// Invoked per leaf by the redactor, not by [`Analyzer::analyze`].
    ner: Option<std::sync::Arc<crate::ner::NerEngine>>,
}

impl Analyzer {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Build an analyzer pre-loaded with the default recognizer registry.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self {
            recognizers: crate::recognizers::all(),
            ner: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn register(&mut self, recognizer: Recognizer) -> &mut Self {
        self.recognizers.push(recognizer);
        self
    }

    /// Analyze `text`, returning merged + overlap-resolved results.
    #[must_use]
    pub fn analyze(&self, text: &str, opts: &AnalyzeOptions) -> Vec<RecognizerResult> {
        self.analyze_with_context(text, &[], opts)
    }

    /// Same as [`Self::analyze`] but threads an external context word list
    /// into the boost step. Crate-private — populated by the redactor's
    /// JSON-key path split into lowercase words. Words must already be
    /// lowercased.
    #[must_use]
    pub(crate) fn analyze_with_context(
        &self,
        text: &str,
        external_context: &[String],
        opts: &AnalyzeOptions,
    ) -> Vec<RecognizerResult> {
        // Apply boost BEFORE the min_score filter so weak candidates can be
        // rescued by a supportive keyword. Then drop anything still below
        // the floor and resolve overlap. Boost runs per recognizer so each
        // batch is paired with its producer — no name-based lookup.
        let raw: Vec<RecognizerResult> = self
            .recognizers
            .iter()
            .flat_map(|rec| {
                let hits = rec.analyze(text);
                match opts.context.as_ref() {
                    Some(settings) => apply_context_boost(text, hits, rec, external_context, settings),
                    None => hits,
                }
            })
            .filter(|r| r.score >= opts.min_score)
            .collect();
        overlap::resolve(raw)
    }

    /// Construct a fresh [`Builder`] for category routing.
    #[must_use]
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Iterate the registry's recognizers in registration order.
    pub fn recognizers(&self) -> impl Iterator<Item = &Recognizer> + '_ {
        self.recognizers.iter()
    }

    /// Resolve a [`PiiConfig`] to an [`Analyzer`].
    ///
    /// - `categories` unset → [`Analyzer::with_defaults`].
    /// - `categories` set → builder filters the registry by category set.
    /// - On builder error, falls back to `with_defaults` and logs at `warn`
    ///   level so a misconfiguration never disables redaction silently.
    #[must_use]
    pub fn from_config(config: &PiiConfig) -> Self {
        let Some(cats) = config.categories.as_ref() else {
            return Self::with_defaults();
        };
        Self::builder()
            .categories(cats.iter().copied().map(map_category))
            .build()
            .unwrap_or_else(|err| {
                tracing::warn!(
                    target: "dbmcp::pii",
                    error = %err,
                    "PII analyzer build failed; falling back to with_defaults()"
                );
                Self::with_defaults()
            })
    }

    /// Borrows the attached NER engine, if one was loaded.
    pub(crate) fn ner_engine(&self) -> Option<&crate::ner::NerEngine> {
        self.ner.as_deref()
    }

    /// Attaches a loaded NER engine for the redactor's per-leaf pass.
    pub(crate) fn attach_ner(&mut self, engine: std::sync::Arc<crate::ner::NerEngine>) {
        self.ner = Some(engine);
    }
}

fn map_category(c: PiiCategory) -> Category {
    match c {
        PiiCategory::Personal => Category::Personal,
        PiiCategory::Financial => Category::Financial,
        PiiCategory::Government => Category::Government,
        PiiCategory::Contact => Category::Contact,
        PiiCategory::Network => Category::Network,
        PiiCategory::DigitalIdentity => Category::DigitalIdentity,
        PiiCategory::Crypto => Category::Crypto,
    }
}

/// Typed builder that filters the `all()` registry by category.
#[derive(Default, Debug)]
pub struct Builder {
    categories: Option<Vec<Category>>,
}

impl Builder {
    /// Set the effective category set the analyzer filters by.
    #[must_use]
    pub fn categories(mut self, cats: impl IntoIterator<Item = Category>) -> Self {
        let mut out: Vec<Category> = Vec::new();
        for c in cats {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        self.categories = Some(out);
        self
    }

    /// Build the [`Analyzer`] applying the resolved filters.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzerBuildError::EmptyCategory`] if a requested category
    /// has zero recognizers tagging it.
    pub fn build(self) -> Result<Analyzer, AnalyzerBuildError> {
        let Some(cats) = self.categories else {
            return Ok(Analyzer::with_defaults());
        };

        let kept: Vec<Recognizer> = crate::recognizers::all()
            .into_iter()
            .filter(|r| cats.contains(&r.category()))
            .collect();

        for &cat in &cats {
            if !kept.iter().any(|r| r.category() == cat) {
                return Err(AnalyzerBuildError::EmptyCategory(cat));
            }
        }

        Ok(Analyzer {
            recognizers: kept,
            ner: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AnalyzeOptions, Analyzer};
    use crate::context::ContextSettings;
    use crate::pattern::Pattern;
    use crate::recognizers::Recognizer;
    use crate::score::Score;
    use crate::validators::Validator;
    use crate::{Entity, MAX_SCORE, ValidationOutcome};

    fn ctx_settings() -> ContextSettings {
        ContextSettings {
            similarity_factor: Score::from_static(0.35),
            min_score_with_context: Score::from_static(0.4),
            prefix_words: 5,
            suffix_words: 0,
        }
    }

    fn analyzer_with(rec: Recognizer) -> Analyzer {
        let mut a = Analyzer::empty();
        a.register(rec);
        a
    }

    #[test]
    fn boost_off_byte_identical_to_baseline() {
        let p = Pattern::new("p", r"\d{3}", Score::from_static(0.3)).expect("static");
        let rec = Recognizer::new(Entity::PhoneNumber, vec![p])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["phone"]);
        let a = analyzer_with(rec);
        let text = "my phone 415";
        let baseline = a.analyze(text, &AnalyzeOptions::default());
        let same = a.analyze(
            text,
            &AnalyzeOptions {
                min_score: Score::default(),
                context: None,
            },
        );
        assert_eq!(baseline, same);
        // No supportive keyword recorded when context disabled.
        assert!(baseline.iter().all(|r| r.explanation.supportive_keyword.is_none()));
    }

    #[test]
    fn boost_lifts_unknown_outcome_to_floor() {
        let p = Pattern::new("p", r"\d{3}", Score::from_static(0.1)).expect("static");
        let rec = Recognizer::new(Entity::PhoneNumber, vec![p])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["phone"]);
        let a = analyzer_with(rec);
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let out = a.analyze("my phone 415", &opts);
        assert_eq!(out.len(), 1);
        assert!(out[0].score.as_f32() >= 0.4);
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn invalid_validator_dropped_not_rescued() {
        // Luhn-fail credit card: 4012-8888-8888-1882 (last digit altered).
        // Without context: dropped by Luhn. With context: still dropped (FR-008).
        let a = Analyzer::with_defaults();
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let out = a.analyze("my card 4012-8888-8888-1882", &opts);
        assert!(
            out.iter().all(|r| r.entity_type != Entity::CreditCard),
            "Luhn-fail credit card must NOT be rescued by context boost"
        );
    }

    #[test]
    fn valid_validator_keeps_max_no_keyword() {
        // Luhn-valid credit card with "card" nearby. Context boost must
        // not claim a supportive keyword because score is already MAX.
        let p = Pattern::new("credit_card", r"\b\d{16}\b", Score::from_static(0.3)).expect("static");
        let rec = Recognizer::new(Entity::CreditCard, vec![p])
            .expect("non-empty")
            .with_name("CreditCardRecognizer")
            .with_validator(Validator::Luhn)
            .with_context(&["card"]);
        let a = analyzer_with(rec);
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let out = a.analyze("my card 4012888888881881", &opts);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].score, MAX_SCORE);
        assert_eq!(out[0].explanation.validation, ValidationOutcome::Valid);
        assert!(out[0].explanation.supportive_keyword.is_none());
    }

    #[test]
    fn analyze_with_context_appends_external_words() {
        let p = Pattern::new("p", r"\d{3}", Score::from_static(0.1)).expect("static");
        let rec = Recognizer::new(Entity::PhoneNumber, vec![p])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["phone"]);
        let a = analyzer_with(rec);
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let external = [String::from("phone")];
        let out = a.analyze_with_context("415", &external, &opts);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn boosted_result_serialises_supportive_keyword() {
        let p = Pattern::new("p", r"\d{3}", Score::from_static(0.1)).expect("static");
        let rec = Recognizer::new(Entity::PhoneNumber, vec![p])
            .expect("non-empty")
            .with_name("PhoneRecognizer")
            .with_context(&["phone"]);
        let a = analyzer_with(rec);
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let out = a.analyze("my phone 415", &opts);
        assert!(!out.is_empty());
        let json = serde_json::to_value(&out[0]).expect("serialise");
        assert_eq!(json["explanation"]["supportive_keyword"], "phone");
        let final_score = json["explanation"]["final_score"].as_f64().expect("score is number");
        assert!(final_score >= 0.4);
    }

    #[test]
    fn validator_promoted_max_omits_supportive_keyword() {
        let p = Pattern::new("p", r"\b\d{16}\b", Score::from_static(0.3)).expect("static");
        let rec = Recognizer::new(Entity::CreditCard, vec![p])
            .expect("non-empty")
            .with_name("CreditCardRecognizer")
            .with_validator(Validator::Luhn)
            .with_context(&["card"]);
        let a = analyzer_with(rec);
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ctx_settings()),
        };
        let out = a.analyze("my card 4012888888881881", &opts);
        assert_eq!(out.len(), 1);
        let json = serde_json::to_value(&out[0]).expect("serialise");
        assert!(
            json["explanation"].get("supportive_keyword").is_none(),
            "validator-promoted MAX score must omit supportive_keyword: {json}"
        );
        assert!((json["explanation"]["final_score"].as_f64().expect("number") - 1.0).abs() < 1e-6);
    }
}
