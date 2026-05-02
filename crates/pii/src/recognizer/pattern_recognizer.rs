//! Generic pattern-driven recognizer with optional checksum/parser validator.

use std::borrow::Cow;
use std::slice;

use super::{EntityType, NoopValidator, Recognizer, ValidationOutcome, Validator};
use crate::analyzer::AnalyzeOptions;
use crate::error::RecognizerError;
use crate::pattern::Pattern;
use crate::result::{AnalysisExplanation, RecognizerResult};
use crate::score::{MAX_SCORE, MIN_SCORE};

/// Pattern-driven recognizer used by every built-in entity type and by user-supplied custom recognizers.
pub struct PatternRecognizer {
    entity_type: EntityType,
    name: Cow<'static, str>,
    patterns: Vec<Pattern>,
    validator: Box<dyn Validator>,
}

impl std::fmt::Debug for PatternRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternRecognizer")
            .field("entity_type", &self.entity_type)
            .field("name", &self.name)
            .field("patterns", &self.patterns)
            .finish_non_exhaustive()
    }
}

impl PatternRecognizer {
    /// Build a recognizer for `entity_type`. Defaults: name `"<EntityType>Recognizer"`, no validator.
    ///
    /// # Errors
    ///
    /// Returns [`RecognizerError::EmptyPatternList`] when `patterns` is empty.
    pub fn new(entity_type: EntityType, patterns: Vec<Pattern>) -> Result<Self, RecognizerError> {
        if patterns.is_empty() {
            return Err(RecognizerError::EmptyPatternList);
        }
        let name = Cow::Owned(format!("{}Recognizer", entity_type.as_str()));
        Ok(Self {
            entity_type,
            name,
            patterns,
            validator: Box::new(NoopValidator),
        })
    }

    /// Override the recognizer's display name (used in [`AnalysisExplanation::recognizer_name`]).
    #[must_use]
    pub fn with_name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = name.into();
        self
    }

    /// Attach a validator hook that runs against every regex match.
    #[must_use]
    pub fn with_validator<V>(mut self, validator: V) -> Self
    where
        V: Validator + 'static,
    {
        self.validator = Box::new(validator);
        self
    }

    fn build_result(&self, pattern: &Pattern, start: usize, end: usize, text: &str) -> Option<RecognizerResult> {
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        let candidate = &text[start..end];
        let validation = self.validator.validate(candidate);
        let original_score = pattern.score();
        let final_score = match validation {
            ValidationOutcome::Valid => MAX_SCORE,
            ValidationOutcome::Invalid => return None,
            ValidationOutcome::Unknown => original_score,
        };
        if final_score == MIN_SCORE {
            return None;
        }
        Some(RecognizerResult {
            entity_type: self.entity_type.clone(),
            start,
            end,
            score: final_score,
            explanation: AnalysisExplanation {
                recognizer_name: self.name.clone(),
                pattern_name: Some(pattern.name_cow()),
                original_score,
                validation,
                final_score,
            },
        })
    }
}

impl Recognizer for PatternRecognizer {
    fn name(&self) -> &str {
        &self.name
    }

    fn supported_entities(&self) -> &[EntityType] {
        slice::from_ref(&self.entity_type)
    }

    fn analyze(&self, text: &str, _opts: &AnalyzeOptions) -> Vec<RecognizerResult> {
        self.patterns
            .iter()
            .flat_map(|pattern| {
                pattern
                    .compiled
                    .find_iter(text)
                    .filter_map(move |m| self.build_result(pattern, m.start(), m.end(), text))
            })
            .collect()
    }
}
