//! Configuration validation errors and their non-empty-collection wrapper.
//!
//! [`ConfigError`] enumerates rule violations found by the loader and by
//! the per-section `validate()` methods. [`ConfigErrors`] is a non-empty
//! collection wrapper returned by every
//! [`DatabaseConfig::validate`] / [`HttpConfig::validate`] /
//! [`PiiConfig::validate`] / [`SshConfig::validate`] call.
//!
//! [`DatabaseConfig::validate`]: crate::DatabaseConfig::validate
//! [`HttpConfig::validate`]: crate::HttpConfig::validate
//! [`PiiConfig::validate`]: crate::PiiConfig::validate
//! [`SshConfig::validate`]: crate::SshConfig::validate

/// Rule violations found while building and validating a [`Config`](crate::Config).
///
/// Each message names the setting by its dotted YAML key path. Missing
/// required settings also name every route that sets them.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `database.name` is required for `SQLite`.
    #[error("database.name (file path) is required for sqlite; set DBMCP_DB_NAME or database.name in the config file")]
    MissingSqliteDbName,

    /// A configured file (SSL certificate, SSH key) was not found.
    #[error("{0}: file not found: {1}")]
    FileNotFound(&'static str, String),

    /// A numeric setting is outside its accepted range.
    #[error("{path} must be {expected}")]
    OutOfRange {
        /// Dotted YAML key path of the setting.
        path: &'static str,
        /// Human-readable accepted range, e.g. `at least 1`.
        expected: &'static str,
    },

    /// A list setting was set to an empty list.
    #[error("{0} must not be empty when set")]
    EmptyList(&'static str),

    /// A list setting repeats an entry.
    #[error("{0}: duplicate entry '{1}'")]
    DuplicateListEntry(&'static str, String),

    /// HTTP bind host is empty or whitespace.
    #[error("http.host must not be empty")]
    EmptyHttpHost,

    /// HTTP bearer token was supplied but is empty or whitespace.
    #[error("http.auth_token must not be empty when set")]
    EmptyHttpAuthToken,

    /// HTTP bind host is not loopback and no bearer token is configured.
    #[error(
        "http.host: refusing to bind non-loopback host '{0}' without authentication; \
         set DBMCP_HTTP_AUTH_TOKEN (http.auth_token) or \
         DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED (http.allow_unauthenticated)"
    )]
    UnauthenticatedNonLoopbackBind(String),

    /// An allowed-origins entry is not a valid origin.
    #[error(
        "http.allowed_origins: entry '{0}' is not a valid origin; \
         expected `<scheme>://<host>[:<port>|:*]` or `null`"
    )]
    InvalidAllowedOrigin(String),

    /// NER was enabled without a model path.
    #[error(
        "pii.ner_model (model directory) is required when pii.ner_enabled is set; \
         set DBMCP_PII_NER_MODEL or pii.ner_model in the config file"
    )]
    PiiNerModelMissing,

    /// SSH tunnelling was requested for the `SQLite` backend.
    #[error("ssh.host: not supported with the sqlite backend")]
    SshSqliteUnsupported,

    /// Other SSH settings were set without `ssh.host`.
    #[error("ssh.host is required when other ssh settings are set; set DBMCP_SSH_HOST or ssh.host in the config file")]
    MissingSshHost,

    /// `ssh.host` was set to an empty string.
    #[error("ssh.host must not be empty")]
    EmptySshHost,

    /// `ssh.host` was set without `ssh.user`.
    #[error("ssh.user is required when ssh.host is set; set DBMCP_SSH_USER or ssh.user in the config file")]
    MissingSshUser,

    /// `ssh.host` was set without a password or key.
    #[error(
        "ssh.password or ssh.key is required when ssh.host is set; \
         set DBMCP_SSH_PASSWORD, DBMCP_SSH_KEY, or either key in the config file"
    )]
    MissingSshAuth,

    /// `ssh.key_passphrase` was set without `ssh.key`.
    #[error("ssh.key_passphrase requires ssh.key")]
    SshKeyPassphraseWithoutKey,

    /// `known_hosts` could not be resolved and host-key checking is on.
    #[error(
        "ssh.known_hosts is required when the home directory cannot be determined; \
         set DBMCP_SSH_KNOWN_HOSTS or ssh.known_hosts in the config file"
    )]
    MissingSshKnownHosts,
}

impl ConfigError {
    /// Returns the dotted key paths of every setting the error involves.
    ///
    /// The loader appends the source of each one that was set.
    #[must_use]
    pub fn paths(&self) -> &[&'static str] {
        match self {
            Self::MissingSqliteDbName => &["database.backend"],
            Self::FileNotFound(path, _) | Self::EmptyList(path) | Self::DuplicateListEntry(path, _) => {
                std::slice::from_ref(path)
            }
            Self::OutOfRange { path, .. } => std::slice::from_ref(path),
            Self::EmptyHttpHost | Self::UnauthenticatedNonLoopbackBind(_) => &["http.host"],
            Self::EmptyHttpAuthToken => &["http.auth_token"],
            Self::InvalidAllowedOrigin(_) => &["http.allowed_origins"],
            Self::PiiNerModelMissing => &["pii.ner_enabled"],
            Self::SshSqliteUnsupported => &["ssh.host", "database.backend"],
            Self::EmptySshHost | Self::MissingSshUser | Self::MissingSshAuth => &["ssh.host"],
            Self::MissingSshHost | Self::MissingSshKnownHosts => &[],
            Self::SshKeyPassphraseWithoutKey => &["ssh.key_passphrase"],
        }
    }
}

/// Reports each entry of `list` that repeats an earlier one.
pub(crate) fn duplicates<'a, T: PartialEq + std::fmt::Display>(
    path: &'static str,
    list: &'a [T],
) -> impl Iterator<Item = ConfigError> + 'a {
    list.iter()
        .enumerate()
        .filter(|(i, entry)| list[..*i].contains(entry))
        .map(move |(_, entry)| ConfigError::DuplicateListEntry(path, entry.to_string()))
}

/// Non-empty collection of [`ConfigError`]s preserving insertion order.
///
/// Externally observed wrappers always carry ≥ 1 error — [`Self::from_vec`]
/// returns `None` for an empty input, and [`From<ConfigError>`] yields a
/// single-element wrapper.
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
        ConfigError::FileNotFound("database.ssl_ca", "/nope/ca.pem".into())
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
        assert!(matches!(errors[1], ConfigError::FileNotFound(_, _)));
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
        assert!(matches!(collected[1], ConfigError::FileNotFound(_, _)));
    }
}
