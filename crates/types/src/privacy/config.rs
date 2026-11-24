//! Privacy settings: mode, operator, categories, and column rules.

use std::path::PathBuf;

use secrecy::SecretString;

use super::{Category, Entity};

/// What privacy protection the query tools apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No privacy section: rows and error messages pass through unchanged.
    None,
    /// Column rules only: ruled columns are rewritten, nothing else is detected.
    Rules,
    /// Column rules plus global detection on every cell and error message.
    Full,
}

impl Mode {
    /// The mode an unset `privacy.mode` resolves to.
    pub const DEFAULT: Self = Self::Full;
}

impl Mode {
    /// Returns the setting value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rules => "rules",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Supported PII redaction operators.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
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

impl Operator {
    /// Returns the setting value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Mask => "mask",
            Self::Redact => "redact",
            Self::Hash => "hash",
        }
    }
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
    /// The entity the column holds.
    pub entity: Entity,
    /// The operator: the rule's own, else `privacy.operator`.
    pub operator: Operator,
}

impl ColumnRule {
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
/// Present in [`crate::Config`] only when [`Self::mode`] is not [`Mode::None`].
#[derive(Clone, Debug)]
pub struct Config {
    /// [`Mode::Rules`] or [`Mode::Full`]; never [`Mode::None`].
    pub mode: Mode,
    /// Which built-in operator rewrites detected spans; the rules' fallback.
    pub operator: Operator,
    /// HMAC key for the `hash` operator, at least 32 bytes; never logged.
    ///
    /// Set only when [`Self::operator`] or a rule's operator is [`Operator::Hash`].
    pub key: Option<SecretString>,
    /// Optional explicit category set for global detection (`full` only).
    ///
    /// Routes the analyzer through `dbmcp_privacy::Analyzer::builder().categories(...)`.
    pub categories: Option<Vec<Category>>,
    /// NER model directory; when set, `full` runs the ML/NER pass.
    ///
    /// Ignored under [`Mode::Rules`].
    pub model: Option<PathBuf>,
    /// Column rules, in file order, one per listed column (YAML-only `privacy.rules`).
    pub rules: Vec<ColumnRule>,
}

impl Default for Config {
    /// A `full` section with the default operator and nothing else set.
    fn default() -> Self {
        Self {
            mode: Mode::Full,
            operator: Self::DEFAULT_OPERATOR,
            key: None,
            categories: None,
            model: None,
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// Default privacy operator when no override is supplied.
    pub const DEFAULT_OPERATOR: Operator = Operator::Hash;
    /// NER confidence floor applied to every detected span.
    pub const DEFAULT_NER_THRESHOLD: f32 = 0.5;
    /// Minimum `privacy.key` length, in bytes.
    pub const MIN_KEY_BYTES: usize = 32;
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
        let privacy = Config::default();
        assert_eq!(privacy.operator, Operator::Hash);
    }

    #[test]
    fn privacy_config_default_is_full_without_a_model() {
        let privacy = Config::default();
        assert_eq!(privacy.mode, Mode::Full);
        assert!(privacy.model.is_none());
    }

    #[test]
    fn privacy_operator_display_lowercase() {
        assert_eq!(Operator::Replace.to_string(), "replace");
        assert_eq!(Operator::Mask.to_string(), "mask");
        assert_eq!(Operator::Redact.to_string(), "redact");
        assert_eq!(Operator::Hash.to_string(), "hash");
    }
}
