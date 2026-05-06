//! Recognizer abstraction, entity-type newtype, and validator hook.

use std::borrow::Cow;
use std::ops::Range;

use super::category::Category;
use crate::analyzer::AnalyzeOptions;
use crate::result::RecognizerResult;

/// Tag identifying the kind of PII a recognizer emits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EntityType(pub(crate) Cow<'static, str>);

impl EntityType {
    /// Build an entity type from any string-like source.
    #[must_use]
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    /// Return the entity-type name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Outcome of running a [`Validator`] on a candidate match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ValidationOutcome {
    /// Validator confirmed the candidate; promote to `MAX_SCORE`.
    Valid,
    /// Validator rejected the candidate; drop the result.
    Invalid,
    /// Validator abstained; leave the score untouched.
    Unknown,
}

impl ValidationOutcome {
    /// Map a boolean check to [`Self::Valid`] / [`Self::Invalid`].
    ///
    /// Use this when a validator's only outcomes are accept/reject — never
    /// abstain. Reduces the `if cond { Valid } else { Invalid }` boilerplate.
    #[must_use]
    pub const fn from_bool(valid: bool) -> Self {
        if valid { Self::Valid } else { Self::Invalid }
    }
}

/// Optional checksum/parser hook attached to a [`super::Pattern`].
pub trait Validator: Send + Sync {
    /// Validate the matched text and return the corresponding [`ValidationOutcome`].
    fn validate(&self, candidate: &str) -> ValidationOutcome;

    /// Validate using surrounding text (for keyword-context aware validators).
    ///
    /// Default impl delegates to [`Validator::validate`], so existing impls keep
    /// working unchanged. Validators that depend on surrounding text — e.g.
    /// [`super::KeywordValidator`] — override this method.
    fn validate_with_context(&self, candidate: &str, full_text: &str, span: Range<usize>) -> ValidationOutcome {
        let _ = (full_text, span);
        self.validate(candidate)
    }
}

/// Abstraction the analyzer engine talks to; future LLM/NER recognizers plug in here.
pub trait Recognizer: Send + Sync {
    /// Recognizer's display name; surfaced in [`crate::AnalysisExplanation`].
    fn name(&self) -> &str;

    /// Entity types this recognizer is capable of emitting.
    fn supported_entities(&self) -> &[EntityType];

    /// Analyze `text` and return the recognizer's own results, pre-overlap.
    fn analyze(&self, text: &str, opts: &AnalyzeOptions) -> Vec<RecognizerResult>;

    /// Top-level PII category this recognizer covers.
    ///
    /// Default impl returns [`Category::Personal`] so external `Recognizer`
    /// impls keep compiling. Built-in recognizers override via
    /// [`super::Pattern::with_category`].
    fn category(&self) -> Category {
        Category::Personal
    }
}
