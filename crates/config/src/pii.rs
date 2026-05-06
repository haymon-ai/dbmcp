//! PII redaction settings and operator enum.

use crate::error::{ConfigError, ConfigErrors};

/// Supported PII redaction operators exposed on the CLI.
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

/// PII categories exposed on the CLI as `--pii-categories <comma-separated>`.
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
#[derive(Clone, Debug, Default)]
pub struct PiiConfig {
    /// Whether the server redacts PII from query tool response payloads.
    pub enabled: bool,
    /// Which built-in operator rewrites detected spans.
    pub operator: PiiOperator,
    /// Optional explicit category set; routes the analyzer through
    /// `dbmcp_pii::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<PiiCategory>>,
}

impl PiiConfig {
    /// Default PII redaction state (off — opt-in only).
    pub const DEFAULT_ENABLED: bool = false;
    /// Default PII operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PiiOperator = PiiOperator::Replace;

    /// Validates this configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] when `categories` is `Some(empty Vec)`. clap
    /// already rejects unknown values for `--pii-categories`, so that check
    /// lives there.
    pub fn validate(&self) -> Result<(), ConfigErrors> {
        let mut errors = Vec::new();
        if let Some(cats) = &self.categories
            && cats.is_empty()
        {
            errors.push(ConfigError::PiiCategoriesEmpty);
        }
        ConfigErrors::from_vec(errors).map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pii_config_default_disabled() {
        let pii = PiiConfig::default();
        assert!(!pii.enabled, "PiiConfig::default().enabled must be false");
    }

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
    fn pii_operator_display_lowercase() {
        assert_eq!(PiiOperator::Replace.to_string(), "replace");
        assert_eq!(PiiOperator::Mask.to_string(), "mask");
        assert_eq!(PiiOperator::Redact.to_string(), "redact");
        assert_eq!(PiiOperator::Hash.to_string(), "hash");
    }
}
