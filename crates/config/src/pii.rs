//! PII redaction settings and operator enum.

use crate::error::ConfigErrors;

/// Supported PII redaction operators exposed on the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PiiOperator {
    /// Replace each detected span with an entity-aware placeholder (default).
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

/// PII redaction settings for query tool responses.
#[derive(Clone, Debug)]
pub struct PiiConfig {
    /// Whether the server redacts PII from query tool response payloads.
    pub enabled: bool,
    /// Which built-in operator rewrites detected spans.
    pub operator: PiiOperator,
}

impl PiiConfig {
    /// Default PII redaction state (off — opt-in only).
    pub const DEFAULT_ENABLED: bool = false;
    /// Default PII operator when no override is supplied.
    pub const DEFAULT_OPERATOR: PiiOperator = PiiOperator::Replace;

    /// Validates this configuration. Reserved for future per-operator rules.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] when a future rule fails. Currently never returns `Err`.
    #[allow(
        clippy::unused_self,
        clippy::unnecessary_wraps,
        reason = "structural placeholder — every section's validate method shares this signature so future per-operator rules slot in without touching call sites"
    )]
    pub fn validate(&self) -> Result<(), ConfigErrors> {
        Ok(())
    }
}

impl Default for PiiConfig {
    fn default() -> Self {
        Self {
            enabled: Self::DEFAULT_ENABLED,
            operator: Self::DEFAULT_OPERATOR,
        }
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
