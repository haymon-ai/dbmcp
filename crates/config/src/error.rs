//! Configuration validation errors and their non-empty-collection wrapper.
//!
//! [`ConfigError`] enumerates rules reachable past parsing.
//! [`ConfigErrors`] is a non-empty collection wrapper returned by
//! [`ConfigLayer::resolve`](crate::ConfigLayer::resolve) and by every
//! [`DatabaseConfig::validate`] / [`HttpConfig::validate`] /
//! [`PiiConfig::validate`] / [`SshConfig::validate`] call.
//!
//! [`DatabaseConfig::validate`]: crate::DatabaseConfig::validate
//! [`HttpConfig::validate`]: crate::HttpConfig::validate
//! [`PiiConfig::validate`]: crate::PiiConfig::validate
//! [`SshConfig::validate`]: crate::SshConfig::validate

/// Rule violations found while validating a resolved [`Config`](crate::Config).
///
/// Type and enum-membership errors are rejected earlier, by clap or the
/// config-file parser, and are not represented here. Every message starts
/// with the config path it is about; [`Self::fields`] lists every config
/// path involved, so callers can say where each value came from.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `database.name` is required for `SQLite`.
    #[error("database.name: required (file path) for the sqlite backend")]
    MissingSqliteDbName,

    /// A configured file (SSL certificate, SSH key) was not found.
    #[error("{field}: file not found: {path}")]
    FileNotFound {
        /// Config path of the setting.
        field: &'static str,
        /// The missing path.
        path: String,
    },

    /// HTTP bind host is empty or whitespace.
    #[error("http.host: must not be empty")]
    EmptyHttpHost,

    /// HTTP bearer token was supplied but is empty or whitespace.
    #[error("http.auth_token: must not be empty when set")]
    EmptyHttpAuthToken,

    /// HTTP bind host is not loopback and no bearer token is configured.
    #[error(
        "http.host: refusing to bind non-loopback host '{0}' without authentication; \
         set http.auth_token or http.allow_unauthenticated"
    )]
    UnauthenticatedNonLoopbackBind(String),

    /// An allowed-origins entry is not a valid origin.
    #[error(
        "http.allowed_origins: entry '{0}' is not a valid origin; \
         expected `<scheme>://<host>[:<port>|:*]` or `null`"
    )]
    InvalidAllowedOrigin(String),

    /// `pii.categories` was supplied but the resolved list is empty.
    #[error("pii.categories: must not be empty when set; remove it to fall back to defaults")]
    PiiCategoriesEmpty,

    /// `pii.ner_enabled` was set without a `pii.ner_model` path.
    #[error("pii.ner_model: required (model directory) when pii.ner_enabled is set")]
    PiiNerModelMissing,

    /// SSH tunnelling was requested for the `SQLite` backend.
    #[error("ssh.host: not supported with the sqlite backend")]
    SshSqliteUnsupported,

    /// Other SSH options were set without `ssh.host`.
    #[error("ssh.host: required when other ssh options are set")]
    MissingSshHost,

    /// `ssh.host` was set to an empty string.
    #[error("ssh.host: must not be empty")]
    EmptySshHost,

    /// `ssh.host` was set without `ssh.user`.
    #[error("ssh.user: required when ssh.host is set")]
    MissingSshUser,

    /// `ssh.host` was set without a password or key.
    #[error("ssh.password: required when ssh.host is set and ssh.key is not")]
    MissingSshAuth,

    /// `ssh.key_passphrase` was set without `ssh.key`.
    #[error("ssh.key_passphrase: requires ssh.key")]
    SshKeyPassphraseWithoutKey,

    /// `known_hosts` could not be resolved and host-key checking is on.
    #[error("ssh.known_hosts: required when the home directory cannot be determined")]
    MissingSshKnownHosts,

    /// A numeric setting is below its minimum.
    #[error("{field}: must be at least {min}")]
    BelowMinimum {
        /// Config path of the setting.
        field: &'static str,
        /// Smallest accepted value.
        min: u64,
    },

    /// A numeric setting is outside its accepted range.
    #[error("{field}: must be between {min} and {max}")]
    OutOfRange {
        /// Config path of the setting.
        field: &'static str,
        /// Smallest accepted value.
        min: u64,
        /// Largest accepted value.
        max: u64,
    },

    /// A list setting contains an empty entry.
    #[error("{0}: must not contain empty entries")]
    EmptyListEntry(&'static str),

    /// A list setting contains the same entry more than once.
    #[error("{field}: contains duplicate entry '{entry}'")]
    DuplicateListEntry {
        /// Config path of the setting.
        field: &'static str,
        /// The repeated entry.
        entry: String,
    },
}

impl ConfigError {
    /// Returns the config paths this error is about; the first is its subject.
    #[must_use]
    pub fn fields(&self) -> Vec<&'static str> {
        match self {
            Self::MissingSqliteDbName => vec!["database.name"],
            Self::FileNotFound { field, .. }
            | Self::BelowMinimum { field, .. }
            | Self::OutOfRange { field, .. }
            | Self::EmptyListEntry(field)
            | Self::DuplicateListEntry { field, .. } => vec![field],
            Self::EmptyHttpHost => vec!["http.host"],
            Self::EmptyHttpAuthToken => vec!["http.auth_token"],
            Self::UnauthenticatedNonLoopbackBind(_) => {
                vec!["http.host", "http.auth_token", "http.allow_unauthenticated"]
            }
            Self::InvalidAllowedOrigin(_) => vec!["http.allowed_origins"],
            Self::PiiCategoriesEmpty => vec!["pii.categories"],
            Self::PiiNerModelMissing => vec!["pii.ner_model", "pii.ner_enabled"],
            Self::SshSqliteUnsupported => vec!["ssh.host", "database.backend"],
            Self::MissingSshHost => crate::layer::SshLayer::PATHS.to_vec(),
            Self::EmptySshHost => vec!["ssh.host"],
            Self::MissingSshUser => vec!["ssh.user"],
            Self::MissingSshAuth => vec!["ssh.password"],
            Self::SshKeyPassphraseWithoutKey => vec!["ssh.key_passphrase", "ssh.key"],
            Self::MissingSshKnownHosts => vec!["ssh.known_hosts"],
        }
    }

    /// Returns one error per problem found in the list setting `field`.
    ///
    /// Reports a single [`Self::EmptyListEntry`] when any entry is blank
    /// and one [`Self::DuplicateListEntry`] per repeated entry.
    pub(crate) fn check_list<T: PartialEq + std::fmt::Display>(
        field: &'static str,
        entries: &[T],
        is_blank: impl Fn(&T) -> bool,
    ) -> Vec<Self> {
        let mut errors = Vec::new();
        if entries.iter().any(&is_blank) {
            errors.push(Self::EmptyListEntry(field));
        }
        for (i, entry) in entries.iter().enumerate() {
            if !is_blank(entry) && !entries[..i].contains(entry) && entries[i + 1..].contains(entry) {
                errors.push(Self::DuplicateListEntry {
                    field,
                    entry: entry.to_string(),
                });
            }
        }
        errors
    }
}

/// Non-empty collection of [`ConfigError`]s preserving insertion order.
///
/// Externally observed wrappers always carry ≥ 1 error — [`Self::from_vec`]
/// returns `None` for an empty input, and [`From<ConfigError>`] yields a
/// single-element wrapper. [`ConfigLayer::resolve`](crate::ConfigLayer::resolve)
/// accumulates across sections (database → http → pii → ssh) and returns
/// `Ok(value)` when nothing was collected, never an empty wrapper.
///
/// Derefs to `&[ConfigError]` so callers can use slice methods directly
/// (`iter`, `len`, indexing). `Display` renders each contained error on its
/// own line, joined with `\n`, in stored order — no header, no count, no
/// trailing newline.
#[derive(Debug, thiserror::Error)]
#[error("{}", ErrorList(&self.0))]
pub struct ConfigErrors(Vec<ConfigError>);

struct ErrorList<'a>(&'a [ConfigError]);

impl std::fmt::Display for ErrorList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, err) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{err}")?;
        }
        Ok(())
    }
}

impl ConfigErrors {
    /// Wraps a non-empty `Vec`. Returns `None` when `errors` is empty.
    #[must_use]
    pub fn from_vec(errors: Vec<ConfigError>) -> Option<Self> {
        if errors.is_empty() { None } else { Some(Self(errors)) }
    }
}

impl std::ops::Deref for ConfigErrors {
    type Target = [ConfigError];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<ConfigError> for ConfigErrors {
    fn from(err: ConfigError) -> Self {
        Self(vec![err])
    }
}

impl IntoIterator for ConfigErrors {
    type Item = ConfigError;
    type IntoIter = std::vec::IntoIter<ConfigError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing_name() -> ConfigError {
        ConfigError::MissingSqliteDbName
    }

    fn missing_ca() -> ConfigError {
        ConfigError::FileNotFound {
            field: "database.ssl_ca",
            path: "/nope/ca.pem".into(),
        }
    }

    #[test]
    fn from_vec_empty_is_none() {
        assert!(ConfigErrors::from_vec(Vec::new()).is_none());
    }

    #[test]
    fn from_vec_non_empty_preserves_order() {
        let errors = ConfigErrors::from_vec(vec![missing_name(), missing_ca()]).expect("non-empty");
        assert_eq!(errors.len(), 2);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(errors[1], ConfigError::FileNotFound { .. }));
    }

    #[test]
    fn from_config_error_yields_single() {
        let errors: ConfigErrors = missing_name().into();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn display_n1_equals_inner_verbatim() {
        let errors: ConfigErrors = missing_name().into();
        assert_eq!(errors.to_string(), missing_name().to_string());
        assert!(!errors.to_string().ends_with('\n'));
    }

    #[test]
    fn display_n2_joined_by_newline_no_header_no_trailing_newline() {
        let errors = ConfigErrors::from_vec(vec![missing_name(), missing_ca()]).expect("non-empty");
        let rendered = errors.to_string();
        assert_eq!(
            rendered,
            format!("{}\n{}", missing_name(), missing_ca()),
            "n=2 must be joined with single \\n, no header, no trailing newline"
        );
        assert!(!rendered.ends_with('\n'));
    }

    #[test]
    fn into_iterator_owned_yields_in_stored_order() {
        let errors = ConfigErrors::from_vec(vec![missing_name(), missing_ca()]).expect("non-empty");
        let collected: Vec<ConfigError> = errors.into_iter().collect();
        assert!(matches!(collected[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(collected[1], ConfigError::FileNotFound { .. }));
    }
}
