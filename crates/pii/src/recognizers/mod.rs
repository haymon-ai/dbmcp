//! Recognizer catalog: region-based grouping (`generic/`, `{usa,gbr,can,eur,deu}/`).
//!
//! [`Recognizer`] is the generic regex/checksum recognizer used by every
//! built-in entity type. [`all`] returns the deterministic registration order.

use std::borrow::Cow;
use std::slice;

use crate::error::RecognizerError;
use crate::pattern::Pattern;
use crate::result::{AnalysisExplanation, RecognizerResult};
use crate::score::{MAX_SCORE, MIN_SCORE};
use crate::validators::Validator;
use crate::{Category, Entity, ValidationOutcome};

pub mod can;
pub mod deu;
pub mod eur;
pub mod gbr;
pub mod generic;
pub mod usa;

// Flat re-exports preserve the `dbmcp_pii::recognizers::<name>` public API.
pub use can::sin_can;
pub use deu::{
    commercial_register_deu, driving_licence_deu, health_insurance_deu, id_card_deu, license_plate_deu,
    lifetime_physician_number_deu, medical_practice_id_deu, passport_deu, postcode_deu, social_security_deu,
    tax_id_deu, tax_number_deu,
};
pub use eur::vat_number_eur;
pub use gbr::{
    bank_account_gbr, driving_licence_gbr, nhs_number_gbr, nino_gbr, passport_gbr, postcode_gbr, sort_code_gbr,
    vehicle_registration_gbr,
};
pub use generic::{
    api_key_aws_secret, api_key_strong, credit_card, crypto, cvv, email, iban, ip_address, jwt_token, mac_address,
    phone_number, private_key, url,
};
pub use usa::{
    bank_account_usa, driver_license_usa, itin_usa, mbi_usa, medical_license_usa, npi_usa, passport_usa,
    routing_number_usa, ssn_usa, tax_id_ein_usa,
};

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
                regex.compiled.find_iter(text).filter_map(move |m| match m {
                    Ok(m) => self.build_result(regex, m.start(), m.end(), text),
                    Err(e) => {
                        tracing::warn!(
                            pattern = %regex.name(),
                            text_len = text.len(),
                            error = %e,
                            "fancy-regex match-time error; skipping pattern",
                        );
                        None
                    }
                })
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

/// Return all built-in recognizers in registration order.
///
/// Order is load-bearing for overlap tie-breaks. The AWS-secret leg of
/// `API_KEY` ships as a separate keyword-context recognizer that shares the
/// `API_KEY` entity type but has a different validator profile.
#[must_use]
pub fn all() -> Vec<Recognizer> {
    vec![
        email(),
        credit_card(),
        iban(),
        ip_address(),
        url(),
        phone_number(),
        crypto(),
        ssn_usa(),
        mac_address(),
        bank_account_gbr(),
        sort_code_gbr(),
        routing_number_usa(),
        cvv(),
        itin_usa(),
        tax_id_ein_usa(),
        nhs_number_gbr(),
        nino_gbr(),
        passport_gbr(),
        passport_usa(),
        sin_can(),
        vat_number_eur(),
        api_key_strong(),
        api_key_aws_secret(),
        jwt_token(),
        private_key(),
        medical_license_usa(),
        bank_account_usa(),
        driver_license_usa(),
        mbi_usa(),
        npi_usa(),
        driving_licence_gbr(),
        postcode_gbr(),
        vehicle_registration_gbr(),
        medical_practice_id_deu(),
        commercial_register_deu(),
        driving_licence_deu(),
        health_insurance_deu(),
        id_card_deu(),
        license_plate_deu(),
        lifetime_physician_number_deu(),
        passport_deu(),
        postcode_deu(),
        social_security_deu(),
        tax_id_deu(),
        tax_number_deu(),
    ]
}
