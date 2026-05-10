//! Recognizer struct plus the built-in catalog shipped by default.
//!
//! [`Recognizer`] is the generic regex/checksum recognizer used by every built-in
//! entity type. The submodules expose pre-configured constructors —
//! eight v1 entries plus the catalog-expansion set — registered in
//! deterministic order so overlap-resolution tie-breaks stay stable.

use std::borrow::Cow;
use std::slice;

use crate::error::RecognizerError;
use crate::pattern::Pattern;
use crate::result::{AnalysisExplanation, RecognizerResult};
use crate::score::{MAX_SCORE, MIN_SCORE};
use crate::validators::Validator;
use crate::{Category, Entity, ValidationOutcome};

mod all;
mod api_key;
mod bank_account_uk;
mod credit_card;
mod crypto;
mod cvv;
mod email;
mod iban;
mod ip;
mod itin;
mod jwt_token;
mod mac_address;
mod nhs_number;
mod nino_uk;
mod passport_uk;
mod passport_us;
mod phone;
mod private_key;
mod routing_number_us;
mod sin_ca;
mod sort_code_uk;
mod tax_id_ein;
mod url;
mod us_ssn;
mod vat_number;

pub use all::all;
pub use api_key::{api_key_aws_secret, api_key_strong};
pub use bank_account_uk::bank_account_uk;
pub use credit_card::credit_card;
pub use crypto::crypto;
pub use cvv::cvv;
pub use email::email;
pub use iban::iban;
pub use ip::ip_address;
pub use itin::itin;
pub use jwt_token::jwt_token;
pub use mac_address::mac_address;
pub use nhs_number::nhs_number;
pub use nino_uk::nino_uk;
pub use passport_uk::passport_uk;
pub use passport_us::passport_us;
pub use phone::phone_number;
pub use private_key::private_key;
pub use routing_number_us::routing_number_us;
pub use sin_ca::sin_ca;
pub use sort_code_uk::sort_code_uk;
pub use tax_id_ein::tax_id_ein;
pub use url::url;
pub use us_ssn::us_ssn;
pub use vat_number::vat_number;

/// Generic regex/checksum recognizer used by every built-in entity type.
#[derive(Debug)]
pub struct Recognizer {
    entity_type: Entity,
    name: Cow<'static, str>,
    regexes: Vec<Pattern>,
    validator: Validator,
    category: Category,
}

impl Recognizer {
    /// Build a recognizer for `entity_type`. Defaults: name `"<Entity>Recognizer"`, no validator.
    ///
    /// # Errors
    ///
    /// Returns [`RecognizerError::EmptyPatternList`] when `regexes` is empty.
    pub fn new(entity_type: Entity, regexes: Vec<Pattern>) -> Result<Self, RecognizerError> {
        if regexes.is_empty() {
            return Err(RecognizerError::EmptyPatternList);
        }
        let name = Cow::Owned(format!("{}Recognizer", entity_type.as_str()));
        Ok(Self {
            entity_type,
            name,
            regexes,
            validator: Validator::Noop,
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
    pub fn with_validator(mut self, validator: Validator) -> Self {
        self.validator = validator;
        self
    }

    /// Tag this recognizer with the given category.
    #[must_use]
    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    /// Recognizer's display name; surfaced in [`crate::AnalysisExplanation`].
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Entity types this recognizer is capable of emitting.
    #[must_use]
    pub fn supported_entities(&self) -> &[Entity] {
        slice::from_ref(&self.entity_type)
    }

    /// Top-level PII category this recognizer covers.
    #[must_use]
    pub fn category(&self) -> Category {
        self.category
    }

    /// Analyze `text` and return the recognizer's own results, pre-overlap.
    #[must_use]
    pub fn analyze(&self, text: &str) -> Vec<RecognizerResult> {
        self.regexes
            .iter()
            .flat_map(|regex| {
                regex
                    .compiled
                    .find_iter(text)
                    .filter_map(move |m| self.build_result(regex, m.start(), m.end(), text))
            })
            .collect()
    }

    fn build_result(&self, regex: &Pattern, start: usize, end: usize, text: &str) -> Option<RecognizerResult> {
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        let candidate = &text[start..end];
        let validation = self.validator.validate_with_context(candidate, text, start..end);
        let original_score = regex.score();
        let final_score = match validation {
            ValidationOutcome::Valid => MAX_SCORE,
            ValidationOutcome::Invalid => return None,
            ValidationOutcome::Unknown => original_score,
        };
        if final_score == MIN_SCORE {
            return None;
        }
        Some(RecognizerResult {
            entity_type: self.entity_type,
            start,
            end,
            score: final_score,
            explanation: AnalysisExplanation {
                recognizer_name: self.name.clone(),
                pattern_name: Some(regex.name_cow()),
                original_score,
                validation,
                final_score,
            },
        })
    }
}
