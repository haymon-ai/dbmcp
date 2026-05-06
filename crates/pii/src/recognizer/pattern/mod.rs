//! Pattern-driven recognizer plus the built-in catalog shipped by default.
//!
//! [`Pattern`] is the generic regex/checksum recognizer used by every built-in
//! entity type and by user-supplied custom recognizers. The submodules expose
//! pre-configured constructors — the eight built-in entries — registered in
//! deterministic order so overlap-resolution tie-breaks stay stable.

use std::borrow::Cow;
use std::slice;

use super::{Category, EntityType, NoopValidator, Recognizer, ValidationOutcome, Validator};
use crate::analyzer::AnalyzeOptions;
use crate::error::RecognizerError;
use crate::regex::Regex;
use crate::result::{AnalysisExplanation, RecognizerResult};
use crate::score::{MAX_SCORE, MIN_SCORE};

mod all;
mod credit_card;
mod crypto;
mod email;
mod iban;
mod ip;
mod phone;
mod url;
mod us_ssn;

pub use all::all;
pub use credit_card::credit_card;
pub use crypto::crypto;
pub use email::email;
pub use iban::iban;
pub use ip::ip_address;
pub use phone::phone_number;
pub use url::url;
pub use us_ssn::us_ssn;

/// Pattern-driven recognizer used by every built-in entity type and by user-supplied custom recognizers.
pub struct Pattern {
    entity_type: EntityType,
    name: Cow<'static, str>,
    patterns: Vec<Regex>,
    validator: Box<dyn Validator>,
    category: Category,
}

impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pattern")
            .field("entity_type", &self.entity_type)
            .field("name", &self.name)
            .field("patterns", &self.patterns)
            .finish_non_exhaustive()
    }
}

impl Pattern {
    /// Build a recognizer for `entity_type`. Defaults: name `"<EntityType>Recognizer"`, no validator.
    ///
    /// # Errors
    ///
    /// Returns [`RecognizerError::EmptyPatternList`] when `patterns` is empty.
    pub fn new(entity_type: EntityType, patterns: Vec<Regex>) -> Result<Self, RecognizerError> {
        if patterns.is_empty() {
            return Err(RecognizerError::EmptyPatternList);
        }
        let name = Cow::Owned(format!("{}Recognizer", entity_type.as_str()));
        Ok(Self {
            entity_type,
            name,
            patterns,
            validator: Box::new(NoopValidator),
            category: Category::Personal,
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

    /// Tag this recognizer with the given category.
    #[must_use]
    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    /// Inherent accessor for the recognizer's category tag.
    #[must_use]
    pub fn category(&self) -> Category {
        self.category
    }

    fn build_result(&self, pattern: &Regex, start: usize, end: usize, text: &str) -> Option<RecognizerResult> {
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        let candidate = &text[start..end];
        let validation = self.validator.validate_with_context(candidate, text, start..end);
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

impl Recognizer for Pattern {
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

    fn category(&self) -> Category {
        self.category
    }
}
