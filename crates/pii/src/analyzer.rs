//! Analyzer engine: registry + entry point + analyze-time options.

use std::collections::HashSet;

use crate::overlap;
use crate::recognizer::{EntityType, Recognizer};
use crate::result::RecognizerResult;
use crate::score::Score;

/// Per-call overrides handed to [`Analyzer::analyze`].
///
/// `min_score` defaults to [`MIN_SCORE`] (via [`Score::default`]); set higher to
/// drop low-confidence matches before overlap resolution.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    /// Restrict the engine to recognizers whose `supported_entities` intersect this set.
    pub entity_allow_list: Option<HashSet<EntityType>>,
    /// Drop results whose score is below this floor before overlap resolution.
    pub min_score: Score,
}

/// Registry of recognizers and the public entry point for PII analysis.
#[derive(Default)]
pub struct Analyzer {
    recognizers: Vec<Box<dyn Recognizer>>,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyzer")
            .field(
                "recognizers",
                &self.recognizers.iter().map(|r| r.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Analyzer {
    /// Build an analyzer with no recognizers; caller registers their own.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build an analyzer pre-loaded with the eight v1 default recognizers.
    #[must_use]
    pub fn with_defaults() -> Self {
        let recognizers = crate::recognizer::builtin::all()
            .into_iter()
            .map(|r| Box::new(r) as Box<dyn Recognizer>)
            .collect();
        Self { recognizers }
    }

    /// Register a recognizer at the end of the registry.
    pub fn register(&mut self, recognizer: Box<dyn Recognizer>) -> &mut Self {
        self.recognizers.push(recognizer);
        self
    }

    /// Analyze `text`, returning merged + overlap-resolved results.
    #[must_use]
    pub fn analyze(&self, text: &str, opts: &AnalyzeOptions) -> Vec<RecognizerResult> {
        let results = self
            .recognizers
            .iter()
            .filter(|r| match &opts.entity_allow_list {
                Some(allow) => r.supported_entities().iter().any(|e| allow.contains(e)),
                None => true,
            })
            .flat_map(|r| r.analyze(text, opts))
            .filter(|r| r.score >= opts.min_score)
            .collect();
        overlap::resolve(results)
    }
}
