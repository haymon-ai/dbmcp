//! PII redaction settings and operator enum.

use std::path::PathBuf;

use crate::env;
use crate::error::{ConfigError, Diagnostic, duplicates, into_result};

/// Supported PII redaction operators.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PiiOperator {
    /// Replace each detected span with an entity-aware placeholder (default).
    #[default]
    Replace,
    /// Mask each detected span with `'*'` (length-preserving).
    Mask,
    /// Remove each detected span (replace with empty string).
    Redact,
    /// Replace each detected span with a stable hex digest (SHA-256).
    Hash,
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
/// Mirror enum of [`dbmcp_pii::Category`]; the binary layer converts. The
/// wire format (kebab-case) lives in `dbmcp-config` so `dbmcp-pii` stays
/// `clap`-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
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

/// PII redaction settings for query tool responses.
///
/// Present in [`crate::Config`] only when redaction is enabled.
#[derive(Clone, Debug, Default)]
pub struct PiiConfig {
    /// Which built-in operator rewrites detected spans.
    pub operator: PiiOperator,
    /// Optional explicit category set; routes the analyzer through
    /// `dbmcp_pii::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<PiiCategory>>,
    /// Whether the optional ML/NER pass (person & location) runs.
    ///
    /// Requires a model path; ignored otherwise. Off by default.
    pub ner_enabled: bool,
    /// Filesystem path to the NER model directory; required when
    /// [`Self::ner_enabled`] is set.
    pub ner_model: Option<PathBuf>,
}

impl PiiConfig {
    /// Default PII operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PiiOperator = PiiOperator::Replace;
    /// Default ML/NER pass state (off — opt-in only).
    pub const DEFAULT_NER_ENABLED: bool = false;
    /// NER confidence floor applied to every detected span.
    pub const DEFAULT_NER_THRESHOLD: f32 = 0.5;

    /// Validates this configuration.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per problem: `categories` is empty or repeats an
    /// entry, or when `ner_enabled` is set without a `ner_model` path.
    pub fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        if let Some(categories) = &self.categories {
            if categories.is_empty() {
                errors
                    .push(Diagnostic::new(ConfigError::EmptyList("pii.categories")).involving(&[env::PII_CATEGORIES]));
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
    fn pii_config_default_operator_is_replace() {
        let pii = PiiConfig::default();
        assert_eq!(pii.operator, PiiOperator::Replace);
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
