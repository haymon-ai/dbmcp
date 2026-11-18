//! PII redaction settings, operator enum, and column rules.

use std::path::PathBuf;

use secrecy::{ExposeSecret, SecretString};

use crate::env;
use crate::error::{ConfigError, Diagnostic, duplicates, into_result};

/// Supported PII redaction operators.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PiiOperator {
    /// Replace each detected span with an entity-aware placeholder.
    Replace,
    /// Mask each detected span with `'*'` (length-preserving).
    Mask,
    /// Remove each detected span (replace with empty string).
    Redact,
    /// Keyed hash token per value (HMAC-SHA-256 under `pii.key`) (default).
    #[default]
    Hash,
}

impl PiiOperator {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[
        ("replace", Self::Replace),
        ("mask", Self::Mask),
        ("redact", Self::Redact),
        ("hash", Self::Hash),
    ];
}

impl std::fmt::Display for PiiOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Replace => write!(f, "replace"),
            Self::Mask => write!(f, "mask"),
            Self::Redact => write!(f, "redact"),
            Self::Hash => write!(f, "hash"),
        }
    }
}

/// PII categories selectable via the `pii.categories` setting.
///
/// Mirror enum of `dbmcp_pii::Category`; the binary layer converts. The
/// wire format (kebab-case) lives in `dbmcp-config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiiCategory {
    /// Personal identifiers: names, emails, DOB.
    Personal,
    /// Financial identifiers: cards, IBANs, bank accounts.
    Financial,
    /// Government IDs: SSN, passport, NHS, NINO, tax IDs.
    Government,
    /// Contact: phone, postal address, postcode.
    Contact,
    /// Network identifiers: IP, URL, MAC.
    Network,
    /// Digital identity: API keys, JWTs, private keys, social handles.
    DigitalIdentity,
    /// Cryptocurrency wallet addresses.
    Crypto,
}

impl PiiCategory {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[
        ("personal", Self::Personal),
        ("financial", Self::Financial),
        ("government", Self::Government),
        ("contact", Self::Contact),
        ("network", Self::Network),
        ("digital-identity", Self::DigitalIdentity),
        ("crypto", Self::Crypto),
    ];
}

impl std::fmt::Display for PiiCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Personal => "personal",
            Self::Financial => "financial",
            Self::Government => "government",
            Self::Contact => "contact",
            Self::Network => "network",
            Self::DigitalIdentity => "digital-identity",
            Self::Crypto => "crypto",
        };
        f.write_str(s)
    }
}

/// One ruled column, flattened from a `pii.rules` entry.
///
/// Each column a rule lists becomes one `ColumnRule` carrying the rule's
/// name, entity, and resolved operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRule {
    /// The rule's name, for messages only; `None` for an unnamed rule.
    pub name: Option<String>,
    /// The `catalog.schema.table.column` key's segments, as written.
    ///
    /// Matching ignores case.
    pub key: [String; 4],
    /// The entity the column holds, one of [`Self::ENTITIES`].
    pub entity: &'static str,
    /// The rule's operator, or `pii.operator` when the rule sets none.
    pub operator: PiiOperator,
}

impl ColumnRule {
    /// Entity names a rule may use; mirrors `dbmcp_pii::Entity::as_str`.
    pub const ENTITIES: [&'static str; 51] = [
        "EMAIL_ADDRESS",
        "CREDIT_CARD",
        "IBAN_CODE",
        "IP_ADDRESS",
        "URL",
        "PHONE_NUMBER",
        "CRYPTO",
        "US_SSN",
        "API_KEY",
        "BANK_ACCOUNT_UK",
        "CVV",
        "ITIN",
        "JWT_TOKEN",
        "MAC_ADDRESS",
        "NHS_NUMBER",
        "NINO_UK",
        "PASSPORT_UK",
        "PASSPORT_US",
        "PRIVATE_KEY",
        "ROUTING_NUMBER_US",
        "SIN_CA",
        "SORT_CODE_UK",
        "TAX_ID_EIN",
        "VAT_NUMBER",
        "MEDICAL_LICENSE_US",
        "BANK_ACCOUNT_US",
        "DRIVER_LICENSE_US",
        "MBI_US",
        "NPI_US",
        "DRIVING_LICENCE_UK",
        "POSTCODE_UK",
        "VEHICLE_REGISTRATION_UK",
        "MEDICAL_PRACTICE_ID_DE",
        "DRIVING_LICENCE_DE",
        "COMMERCIAL_REGISTER_DE",
        "HEALTH_INSURANCE_DE",
        "ID_CARD_DE",
        "LICENSE_PLATE_DE",
        "LIFETIME_PHYSICIAN_NUMBER_DE",
        "PASSPORT_DE",
        "POSTCODE_DE",
        "SOCIAL_SECURITY_DE",
        "TAX_ID_DE",
        "TAX_NUMBER_DE",
        "DATE_OF_BIRTH",
        "PASSWORD_HASH",
        "PERSON",
        "LOCATION",
        "ORGANIZATION",
        "NATIONALITY_RELIGION_POLITICS",
        "FACILITY",
    ];
}

/// PII redaction settings for query tool responses.
///
/// Present in [`crate::Config`] only when redaction is enabled.
#[derive(Clone, Debug, Default)]
pub struct PiiConfig {
    /// Which built-in operator rewrites detected spans.
    pub operator: PiiOperator,
    /// HMAC key for the `hash` operator, at least 32 bytes; never logged.
    ///
    /// Set only when [`Self::operator`] or a column rule's operator is
    /// [`PiiOperator::Hash`].
    pub key: Option<SecretString>,
    /// Optional explicit category set; routes the analyzer through
    /// `dbmcp_pii::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<PiiCategory>>,
    /// Whether the optional ML/NER pass (person, location, organization, NRP, facility) runs.
    ///
    /// Requires a model path; ignored otherwise. Off by default.
    pub ner_enabled: bool,
    /// Filesystem path to the NER model directory; required when
    /// [`Self::ner_enabled`] is set.
    pub ner_model: Option<PathBuf>,
    /// Column rules from `pii.rules`, one per listed column, in file order.
    pub rules: Vec<ColumnRule>,
}

impl PiiConfig {
    /// Default PII operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PiiOperator = PiiOperator::Hash;
    /// Default ML/NER pass state (off — opt-in only).
    pub const DEFAULT_NER_ENABLED: bool = false;
    /// NER confidence floor applied to every detected span.
    pub const DEFAULT_NER_THRESHOLD: f32 = 0.5;
    /// Minimum `pii.key` length, in bytes.
    pub const MIN_KEY_BYTES: usize = 32;

    /// Validates this configuration.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per problem: `key` is blank or shorter than
    /// [`Self::MIN_KEY_BYTES`], `categories` is empty or repeats an entry, or
    /// `ner_enabled` is set without a `ner_model` path.
    pub(crate) fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        if let Some(key) = &self.key {
            let key = key.expose_secret();
            if key.trim().is_empty() {
                errors.push(Diagnostic::new(ConfigError::EmptyPiiKey).involving(&[env::PII_KEY]));
            } else if key.len() < Self::MIN_KEY_BYTES {
                errors.push(Diagnostic::new(ConfigError::PiiKeyTooShort(key.len())).involving(&[env::PII_KEY]));
            }
        }
        if let Some(categories) = &self.categories {
            if categories.is_empty() {
                errors.push(
                    Diagnostic::new(ConfigError::EmptyList(env::PII_CATEGORIES.path)).involving(&[env::PII_CATEGORIES]),
                );
            }
            errors.extend(duplicates(env::PII_CATEGORIES, categories));
        }
        if self.ner_enabled && self.ner_model.is_none() {
            errors.push(
                Diagnostic::new(ConfigError::PiiNerModelMissing)
                    .involving(&[env::PII_NER_ENABLE])
                    .missing(&[env::PII_NER_MODEL]),
            );
        }
        into_result(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pii_config_default_operator_is_hash() {
        let pii = PiiConfig::default();
        assert_eq!(pii.operator, PiiOperator::Hash);
    }

    #[test]
    fn default_config_validates_ok() {
        PiiConfig::default()
            .validate()
            .expect("rule-free section must accept defaults");
    }

    #[test]
    fn pii_config_default_ner_disabled() {
        assert!(!PiiConfig::default().ner_enabled);
    }

    #[test]
    fn ner_enabled_without_model_errors() {
        let cfg = PiiConfig {
            ner_enabled: true,
            ..PiiConfig::default()
        };
        let errors = cfg.validate().expect_err("ner without model must error");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::PiiNerModelMissing)),
            "expected PiiNerModelMissing in {errors:?}"
        );
    }

    #[test]
    fn ner_enabled_with_model_validates_ok() {
        let cfg = PiiConfig {
            ner_enabled: true,
            ner_model: Some(PathBuf::from("/models/ner")),
            ..PiiConfig::default()
        };
        cfg.validate().expect("ner with model path must validate");
    }

    #[test]
    fn pii_operator_display_lowercase() {
        assert_eq!(PiiOperator::Replace.to_string(), "replace");
        assert_eq!(PiiOperator::Mask.to_string(), "mask");
        assert_eq!(PiiOperator::Redact.to_string(), "redact");
        assert_eq!(PiiOperator::Hash.to_string(), "hash");
    }
}
