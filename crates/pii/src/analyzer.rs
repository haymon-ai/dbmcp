//! Analyzer engine: registry + entry point + analyze-time options.

use dbmcp_config::{PiiCategory, PiiConfig};

use crate::error::AnalyzerBuildError;
use crate::overlap;
use crate::recognizer::{Category, Rule};
use crate::result::RecognizerResult;
use crate::score::Score;

/// Per-call overrides handed to [`Analyzer::analyze`].
///
/// `min_score` defaults to [`crate::MIN_SCORE`] (via [`Score::default`]); set higher to
/// drop low-confidence matches before overlap resolution.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    /// Drop results whose score is below this floor before overlap resolution.
    pub min_score: Score,
}

/// Registry of recognizers and the public entry point for PII analysis.
#[derive(Debug, Default)]
pub struct Analyzer {
    recognizers: Vec<Rule>,
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
            recognizers: crate::recognizer::rule::all(),
        }
    }

    #[cfg(test)]
    pub(crate) fn register(&mut self, rule: Rule) -> &mut Self {
        self.recognizers.push(rule);
        self
    }

    /// Analyze `text`, returning merged + overlap-resolved results.
    #[must_use]
    pub fn analyze(&self, text: &str, opts: &AnalyzeOptions) -> Vec<RecognizerResult> {
        let results = self
            .recognizers
            .iter()
            .flat_map(|r| r.analyze(text))
            .filter(|r| r.score >= opts.min_score)
            .collect();
        overlap::resolve(results)
    }

    /// Construct a fresh [`Builder`] for category routing.
    #[must_use]
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Iterate the registry's rules in registration order.
    pub fn recognizers(&self) -> impl Iterator<Item = &Rule> + '_ {
        self.recognizers.iter()
    }

    /// Resolve a [`PiiConfig`] to an [`Analyzer`].
    ///
    /// - `categories` unset → [`Analyzer::with_defaults`].
    /// - `categories` set → builder filters the registry by category set.
    /// - On builder error, falls back to `with_defaults` and logs at `warn`
    ///   level so a misconfiguration never disables redaction silently.
    #[must_use]
    pub fn from_pii_config(config: &PiiConfig) -> Self {
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

        let kept: Vec<Rule> = crate::recognizer::rule::all()
            .into_iter()
            .filter(|r| cats.contains(&r.category()))
            .collect();

        for &cat in &cats {
            if !kept.iter().any(|r| r.category() == cat) {
                return Err(AnalyzerBuildError::EmptyCategory(cat));
            }
        }

        Ok(Analyzer { recognizers: kept })
    }
}
