//! PII redaction settings, operator enum, and column rules.

use std::path::PathBuf;

use secrecy::{ExposeSecret, SecretString};

use crate::env;
use crate::error::{ConfigError, Diagnostic, duplicates, into_result};

/// Supported PII redaction operators.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyOperator {
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

impl PrivacyOperator {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[
        ("replace", Self::Replace),
        ("mask", Self::Mask),
        ("redact", Self::Redact),
        ("hash", Self::Hash),
    ];
}

impl std::fmt::Display for PrivacyOperator {
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
/// Mirror enum of `dbmcp_privacy::Category`; the binary layer converts. The
/// wire format (kebab-case) lives in `dbmcp-config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyCategory {
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

impl PrivacyCategory {
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

impl std::fmt::Display for PrivacyCategory {
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

/// One ruled column, flattened from a named `pii.rules` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRule {
    /// The rule's name, for messages only.
    pub name: String,
    /// The column's position in its rule's `columns` list, for messages only.
    pub item: usize,
    /// `catalog.schema.table.column`, each segment folded by [`ColumnRule::fold`].
    pub key: [String; 4],
    /// The entity the column holds, one of [`ColumnRule::ENTITIES`].
    pub entity: &'static str,
    /// The operator: the rule's own, else `pii.operator`.
    pub operator: PrivacyOperator,
}

impl ColumnRule {
    /// Entity names a rule may use, in `dbmcp_privacy::Entity::ALL` order.
    ///
    /// Mirrors `dbmcp_privacy::Entity::as_str`; a test in `dbmcp-privacy` ties the two.
    pub const ENTITIES: [&'static str; 52] = [
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
        "POSTCODE",
        "DATE_OF_BIRTH",
        "PASSWORD_HASH",
        "PERSON",
        "LOCATION",
        "ORGANIZATION",
        "NATIONALITY_RELIGION_POLITICS",
        "FACILITY",
    ];

    /// Folds a key segment for matching; the only fold rule keys use.
    ///
    /// Keys match case-insensitively, so the loader folds rule keys with it
    /// and a caller must fold catalog names the same way before comparing.
    #[must_use]
    pub fn fold(segment: &str) -> String {
        segment.to_lowercase()
    }
}

/// PII redaction settings for query tool responses.
///
/// Present in [`crate::Config`] only when redaction is enabled.
#[derive(Clone, Debug, Default)]
pub struct PrivacyConfig {
    /// Which built-in operator rewrites detected spans.
    pub operator: PrivacyOperator,
    /// HMAC key for the `hash` operator, at least 32 bytes; never logged.
    ///
    /// Set only when [`Self::operator`] or a rule's operator is [`PrivacyOperator::Hash`].
    pub key: Option<SecretString>,
    /// Optional explicit category set; routes the analyzer through
    /// `dbmcp_privacy::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<PrivacyCategory>>,
    /// Whether the optional ML/NER pass (person, location, organization, NRP, facility) runs.
    ///
    /// Requires a model path; ignored otherwise. Off by default.
    pub ner_enabled: bool,
    /// Filesystem path to the NER model directory; required when
    /// [`Self::ner_enabled`] is set.
    pub ner_model: Option<PathBuf>,
    /// Column rules, in file order, one per listed column (YAML-only `pii.rules`).
    pub rules: Vec<ColumnRule>,
}

impl PrivacyConfig {
    /// Default PII operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PrivacyOperator = PrivacyOperator::Hash;
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
                errors.push(Diagnostic::new(ConfigError::EmptyPrivacyKey).involving(&[env::PRIVACY_KEY]));
            } else if key.len() < Self::MIN_KEY_BYTES {
                errors.push(Diagnostic::new(ConfigError::PrivacyKeyTooShort(key.len())).involving(&[env::PRIVACY_KEY]));
            }
        }
        if let Some(categories) = &self.categories {
            if categories.is_empty() {
                errors.push(
                    Diagnostic::new(ConfigError::EmptyList(env::PRIVACY_CATEGORIES.path))
                        .involving(&[env::PRIVACY_CATEGORIES]),
                );
            }
            errors.extend(duplicates(env::PRIVACY_CATEGORIES, categories));
        }
        if self.ner_enabled && self.ner_model.is_none() {
            errors.push(
                Diagnostic::new(ConfigError::PrivacyNerModelMissing)
                    .involving(&[env::PRIVACY_NER_ENABLE])
                    .missing(&[env::PRIVACY_NER_MODEL]),
            );
        }
        into_result(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_lowercases_every_character() {
        assert_eq!(ColumnRule::fold("Shop_Users"), "shop_users");
        assert_eq!(ColumnRule::fold("ÄRZTE"), "ärzte");
    }

    #[test]
    fn privacy_config_default_operator_is_hash() {
        let privacy = PrivacyConfig::default();
        assert_eq!(privacy.operator, PrivacyOperator::Hash);
    }

    #[test]
    fn default_config_validates_ok() {
        PrivacyConfig::default()
            .validate()
            .expect("rule-free section must accept defaults");
    }

    #[test]
    fn privacy_config_default_ner_disabled() {
        assert!(!PrivacyConfig::default().ner_enabled);
    }

    #[test]
    fn ner_enabled_without_model_errors() {
        let cfg = PrivacyConfig {
            ner_enabled: true,
            ..PrivacyConfig::default()
        };
        let errors = cfg.validate().expect_err("ner without model must error");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::PrivacyNerModelMissing)),
            "expected PrivacyNerModelMissing in {errors:?}"
        );
    }

    #[test]
    fn ner_enabled_with_model_validates_ok() {
        let cfg = PrivacyConfig {
            ner_enabled: true,
            ner_model: Some(PathBuf::from("/models/ner")),
            ..PrivacyConfig::default()
        };
        cfg.validate().expect("ner with model path must validate");
    }

    #[test]
    fn privacy_operator_display_lowercase() {
        assert_eq!(PrivacyOperator::Replace.to_string(), "replace");
        assert_eq!(PrivacyOperator::Mask.to_string(), "mask");
        assert_eq!(PrivacyOperator::Redact.to_string(), "redact");
        assert_eq!(PrivacyOperator::Hash.to_string(), "hash");
    }
}
