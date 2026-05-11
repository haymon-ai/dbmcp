//! Recognizer catalog: region-based grouping (`generic/`, `{us,uk,ca,eu}/`).
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

pub mod ca;
pub mod de;
pub mod eu;
pub mod generic;
pub mod uk;
pub mod us;

// Flat re-exports preserve the `dbmcp_pii::recognizers::<name>` public API.
pub use ca::sin_ca;
pub use de::{
    commercial_register_de, driving_licence_de, health_insurance_de, id_card_de, license_plate_de,
    lifetime_physician_number_de, medical_practice_id_de, passport_de, postcode_de, social_security_de, tax_id_de,
    tax_number_de,
};
pub use eu::vat_number;
pub use generic::{
    api_key_aws_secret, api_key_strong, credit_card, crypto, cvv, email, iban, ip_address, jwt_token, mac_address,
    phone_number, private_key, url,
};
pub use uk::{
    bank_account_uk, driving_licence_uk, nhs_number, nino_uk, passport_uk, postcode_uk, sort_code_uk,
    vehicle_registration_uk,
};
pub use us::{
    bank_account_us, driver_license_us, itin, mbi_us, medical_license_us, npi_us, passport_us, routing_number_us,
    tax_id_ein, us_ssn,
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
        us_ssn(),
        mac_address(),
        bank_account_uk(),
        sort_code_uk(),
        routing_number_us(),
        cvv(),
        itin(),
        tax_id_ein(),
        nhs_number(),
        nino_uk(),
        passport_uk(),
        passport_us(),
        sin_ca(),
        vat_number(),
        api_key_strong(),
        api_key_aws_secret(),
        jwt_token(),
        private_key(),
        medical_license_us(),
        bank_account_us(),
        driver_license_us(),
        mbi_us(),
        npi_us(),
        driving_licence_uk(),
        postcode_uk(),
        vehicle_registration_uk(),
        medical_practice_id_de(),
        commercial_register_de(),
        driving_licence_de(),
        health_insurance_de(),
        id_card_de(),
        license_plate_de(),
        lifetime_physician_number_de(),
        passport_de(),
        postcode_de(),
        social_security_de(),
        tax_id_de(),
        tax_number_de(),
    ]
}
