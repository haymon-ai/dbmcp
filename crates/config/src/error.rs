//! Configuration problems: what went wrong and which settings it involves.
//!
//! [`ConfigError`] says what is wrong. [`Diagnostic`] wraps one with the
//! settings its producer knows are involved: the ones whose values caused
//! it and the required ones that are unset. Cast errors from the loader and
//! rule violations from every section's `validate()` share this one type;
//! the loader attaches each involved value's [`Source`] before rendering.

use std::collections::HashMap;
use std::fmt;

use crate::env::Setting;
use crate::source::Source;

/// What is wrong with the configuration.
///
/// Each message names the setting by its dotted YAML key path.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A value could not be cast to its setting's type.
    #[error("{path}: {reason}")]
    InvalidValue {
        /// Dotted YAML key path of the setting.
        path: &'static str,
        /// Value-free reason, e.g. `expected integer, found string`.
        reason: String,
    },

    /// `database.name` is required for `SQLite`.
    #[error("database.name (file path) is required for sqlite")]
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
        expected: String,
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
    #[error("http.host: refusing to bind non-loopback host '{0}' without authentication")]
    UnauthenticatedNonLoopbackBind(String),

    /// An allowed-origins entry is not a valid origin.
    #[error(
        "http.allowed_origins: entry '{0}' is not a valid origin; \
         expected `<scheme>://<host>[:<port>|:*]` or `null`"
    )]
    InvalidAllowedOrigin(String),

    /// NER was enabled without a model path.
    #[error("pii.ner_model (model directory) is required when pii.ner_enabled is set")]
    PiiNerModelMissing,

    /// SSH tunnelling was requested for the `SQLite` backend.
    #[error("ssh.host: not supported with the sqlite backend")]
    SshSqliteUnsupported,

    /// Other SSH settings were set without `ssh.host`.
    #[error("ssh.host is required when other ssh settings are set")]
    MissingSshHost,

    /// `ssh.host` was set to an empty string.
    #[error("ssh.host must not be empty")]
    EmptySshHost,

    /// `ssh.host` was set without `ssh.user`.
    #[error("ssh.user is required when ssh.host is set")]
    MissingSshUser,

    /// `ssh.host` was set without a password or key.
    #[error("ssh.password or ssh.key is required when ssh.host is set")]
    MissingSshAuth,

    /// `ssh.key_passphrase` was set without `ssh.key`.
    #[error("ssh.key_passphrase requires ssh.key")]
    SshKeyPassphraseWithoutKey,

    /// `known_hosts` could not be resolved and host-key checking is on.
    #[error("ssh.known_hosts is required when the home directory cannot be determined")]
    MissingSshKnownHosts,
}

/// One configuration problem plus the settings its producer says are involved.
///
/// Renders as the error, then a hint naming every way to set the
/// [`missing`](Self::missing) settings, then the source of each involved
/// setting that has one, e.g. `… ; set DBMCP_DB_NAME or database.name in the
/// config file (from DBMCP_DB_BACKEND)`.
#[derive(Debug)]
pub struct Diagnostic {
    /// What is wrong.
    pub error: ConfigError,
    /// Settings whose values caused the problem, each with its source once known.
    pub involved: Vec<(Setting, Option<Source>)>,
    /// Required settings that are unset; any one of them fixes the problem.
    pub missing: Vec<Setting>,
}

impl Diagnostic {
    /// Wraps `error` with no involved or missing settings.
    #[must_use]
    pub fn new(error: ConfigError) -> Self {
        Self {
            error,
            involved: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Adds settings whose values caused the problem.
    #[must_use]
    pub fn involving(mut self, settings: &[Setting]) -> Self {
        self.involved.extend(settings.iter().map(|setting| (*setting, None)));
        self
    }

    /// Adds required settings that are unset.
    #[must_use]
    pub fn missing(mut self, settings: &[Setting]) -> Self {
        self.missing.extend_from_slice(settings);
        self
    }

    /// Fills each involved setting's source from `sources`, keeping known ones.
    pub fn attach_sources(&mut self, sources: &HashMap<&'static str, Source>) {
        for (setting, source) in &mut self.involved {
            if source.is_none() {
                *source = sources.get(setting.path).cloned();
            }
        }
    }
}

impl From<ConfigError> for Diagnostic {
    fn from(error: ConfigError) -> Self {
        Self::new(error)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;
        match self.missing.as_slice() {
            [] => {}
            [setting] => write!(f, "; set {} or {} in the config file", setting.env, setting.path)?,
            many => {
                let routes: Vec<String> = many
                    .iter()
                    .map(|setting| format!("{} ({})", setting.env, setting.path))
                    .collect();
                write!(f, "; set one of {}", routes.join(", "))?;
            }
        }
        let sourced: Vec<(&str, &Source)> = self
            .involved
            .iter()
            .filter_map(|(setting, source)| source.as_ref().map(|source| (setting.path, source)))
            .collect();
        match sourced.as_slice() {
            [] => Ok(()),
            [(_, source)] if self.involved.len() == 1 => write!(f, " (from {source})"),
            many => {
                let list: Vec<String> = many
                    .iter()
                    .map(|(path, source)| format!("{path} from {source}"))
                    .collect();
                write!(f, " ({})", list.join("; "))
            }
        }
    }
}

/// Converts collected diagnostics into a `validate()` result.
///
/// # Errors
///
/// Returns `diagnostics` when it is not empty.
pub(crate) fn into_result(diagnostics: Vec<Diagnostic>) -> Result<(), Vec<Diagnostic>> {
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

/// Reports each entry of `list` that repeats an earlier one.
pub(crate) fn duplicates<T: PartialEq + fmt::Display>(
    setting: Setting,
    list: &[T],
) -> impl Iterator<Item = Diagnostic> + '_ {
    list.iter()
        .enumerate()
        .filter(|(i, entry)| list[..*i].contains(entry))
        .map(move |(_, entry)| {
            Diagnostic::new(ConfigError::DuplicateListEntry(setting.path, entry.to_string())).involving(&[setting])
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env;

    fn env_source(name: &'static str) -> Source {
        Source::Env(name)
    }

    #[test]
    fn bare_diagnostic_renders_the_error() {
        let diagnostic = Diagnostic::new(ConfigError::EmptySshHost);
        assert_eq!(diagnostic.to_string(), "ssh.host must not be empty");
    }

    #[test]
    fn single_missing_setting_names_env_and_key() {
        let diagnostic = Diagnostic::new(ConfigError::MissingSqliteDbName).missing(&[env::DB_NAME]);
        assert_eq!(
            diagnostic.to_string(),
            "database.name (file path) is required for sqlite; set DBMCP_DB_NAME or database.name in the config file"
        );
    }

    #[test]
    fn several_missing_settings_list_every_route() {
        let diagnostic = Diagnostic::new(ConfigError::MissingSshAuth).missing(&[env::SSH_PASSWORD, env::SSH_KEY]);
        assert!(
            diagnostic
                .to_string()
                .ends_with("; set one of DBMCP_SSH_PASSWORD (ssh.password), DBMCP_SSH_KEY (ssh.key)")
        );
    }

    #[test]
    fn one_involved_source_renders_as_from() {
        let mut diagnostic = Diagnostic::new(ConfigError::EmptyHttpHost).involving(&[env::HTTP_HOST]);
        diagnostic.attach_sources(&HashMap::from([("http.host", env_source("DBMCP_HTTP_HOST"))]));
        assert_eq!(
            diagnostic.to_string(),
            "http.host must not be empty (from DBMCP_HTTP_HOST)"
        );
    }

    #[test]
    fn several_involved_sources_are_listed_per_setting() {
        let mut diagnostic =
            Diagnostic::new(ConfigError::SshSqliteUnsupported).involving(&[env::SSH_HOST, env::DB_BACKEND]);
        diagnostic.attach_sources(&HashMap::from([
            ("ssh.host", env_source("DBMCP_SSH_HOST")),
            ("database.backend", env_source("DBMCP_DB_BACKEND")),
        ]));
        assert_eq!(
            diagnostic.to_string(),
            "ssh.host: not supported with the sqlite backend \
             (ssh.host from DBMCP_SSH_HOST; database.backend from DBMCP_DB_BACKEND)"
        );
    }

    #[test]
    fn involved_settings_without_a_source_are_skipped() {
        let mut diagnostic =
            Diagnostic::new(ConfigError::SshSqliteUnsupported).involving(&[env::SSH_HOST, env::DB_BACKEND]);
        diagnostic.attach_sources(&HashMap::from([("ssh.host", env_source("DBMCP_SSH_HOST"))]));
        assert!(diagnostic.to_string().ends_with("(ssh.host from DBMCP_SSH_HOST)"));
    }

    #[test]
    fn attach_keeps_sources_already_known() {
        let mut diagnostic = Diagnostic::new(ConfigError::EmptyHttpHost);
        diagnostic.involved.push((env::HTTP_HOST, Some(env_source("FIRST"))));
        diagnostic.attach_sources(&HashMap::from([("http.host", env_source("SECOND"))]));
        assert!(diagnostic.to_string().ends_with("(from FIRST)"));
    }

    #[test]
    fn duplicates_report_each_repeat_once() {
        let list = ["a", "b", "a", "a"];
        let found: Vec<String> = duplicates(env::HTTP_ALLOWED_HOSTS, &list)
            .map(|d| d.error.to_string())
            .collect();
        assert_eq!(found, ["http.allowed_hosts: duplicate entry 'a'"; 2]);
    }
}
