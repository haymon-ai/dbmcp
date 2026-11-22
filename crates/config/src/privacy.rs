//! Privacy settings: mode, operator, categories, and column rules.

use std::path::PathBuf;

use secrecy::SecretString;

use crate::env;
use crate::error::{ConfigError, Diagnostic, duplicates, into_result};

/// What privacy protection the query tools apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyMode {
    /// No privacy section: rows and error messages pass through unchanged.
    None,
    /// Column rules only: ruled columns are rewritten, nothing else is detected.
    Rules,
    /// Column rules plus global detection on every cell and error message.
    Full,
}

impl PrivacyMode {
    /// The mode an unset `privacy.mode` resolves to.
    pub const DEFAULT: Self = Self::None;

    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[("none", Self::None), ("rules", Self::Rules), ("full", Self::Full)];
}

impl std::fmt::Display for PrivacyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Rules => write!(f, "rules"),
            Self::Full => write!(f, "full"),
        }
    }
}

/// Supported PII redaction operators.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyOperator {
    /// Replace each detected span with an entity-aware placeholder.
    Replace,
    /// Mask each detected span with `'*'` (length-preserving).
    Mask,
    /// Remove each detected span (replace with empty string).
    Redact,
    /// Keyed hash token per value (HMAC-SHA-256 under `privacy.key`) (default).
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

/// PII categories selectable via the `privacy.categories` setting.
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

/// One ruled column, flattened from a named `privacy.rules` entry.
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
    /// The operator: the rule's own, else `privacy.operator`.
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

/// Privacy settings for query tool responses.
///
/// Present in [`crate::Config`] only when [`Self::mode`] is not [`PrivacyMode::None`].
#[derive(Clone, Debug)]
pub struct PrivacyConfig {
    /// [`PrivacyMode::Rules`] or [`PrivacyMode::Full`]; never [`PrivacyMode::None`].
    pub mode: PrivacyMode,
    /// Which built-in operator rewrites detected spans; the rules' fallback.
    pub operator: PrivacyOperator,
    /// HMAC key for the `hash` operator, at least 32 bytes; never logged.
    ///
    /// Set only when [`Self::operator`] or a rule's operator is [`PrivacyOperator::Hash`].
    pub key: Option<SecretString>,
    /// Optional explicit category set for global detection (`full` only).
    ///
    /// Routes the analyzer through `dbmcp_privacy::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<PrivacyCategory>>,
    /// NER model directory; when set, `full` runs the ML/NER pass.
    ///
    /// Ignored under [`PrivacyMode::Rules`].
    pub model: Option<PathBuf>,
    /// Column rules, in file order, one per listed column (YAML-only `privacy.rules`).
    pub rules: Vec<ColumnRule>,
}

impl Default for PrivacyConfig {
    /// A `full` section with the default operator and nothing else set.
    fn default() -> Self {
        Self {
            mode: PrivacyMode::Full,
            operator: Self::DEFAULT_OPERATOR,
            key: None,
            categories: None,
            model: None,
            rules: Vec::new(),
        }
    }
}

impl PrivacyConfig {
    /// Default privacy operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PrivacyOperator = PrivacyOperator::Hash;
    /// NER confidence floor applied to every detected span.
    pub const DEFAULT_NER_THRESHOLD: f32 = 0.5;
    /// Minimum `privacy.key` length, in bytes.
    pub const MIN_KEY_BYTES: usize = 32;

    /// Validates this configuration.
    ///
    /// The key is checked by the loader, in every mode, so not here.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per problem: `categories` is empty or
    /// repeats an entry.
    pub(crate) fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        if let Some(categories) = &self.categories {
            if categories.is_empty() {
                errors.push(
                    Diagnostic::new(ConfigError::EmptyList(env::PRIVACY_CATEGORIES.path))
                        .involving(&[env::PRIVACY_CATEGORIES]),
                );
            }
            errors.extend(duplicates(env::PRIVACY_CATEGORIES, categories));
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
    fn privacy_config_default_is_full_without_a_model() {
        let privacy = PrivacyConfig::default();
        assert_eq!(privacy.mode, PrivacyMode::Full);
        assert!(privacy.model.is_none());
    }

    #[test]
    fn privacy_mode_display_matches_its_choices() {
        for (name, mode) in PrivacyMode::CHOICES {
            assert_eq!(&mode.to_string(), name);
        }
    }

    #[test]
    fn privacy_operator_display_lowercase() {
        assert_eq!(PrivacyOperator::Replace.to_string(), "replace");
        assert_eq!(PrivacyOperator::Mask.to_string(), "mask");
        assert_eq!(PrivacyOperator::Redact.to_string(), "redact");
        assert_eq!(PrivacyOperator::Hash.to_string(), "hash");
    }
}
