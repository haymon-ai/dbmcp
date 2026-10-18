//! Where each configuration value came from, and how a user can set it.
//!
//! [`Provenance`] maps config paths (`database.port`) to the [`Source`] of
//! the value that won the merge and to the CLI [`Route`] that sets it. It
//! turns a [`ConfigError`] into a note naming those sources, or, for a
//! setting nobody set, every way to set it.

use std::collections::HashMap;

use crate::error::ConfigError;

/// Origin of one merged configuration value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A command-line flag, e.g. `--db-port`.
    Flag(String),
    /// A process env var, e.g. `DB_PORT`.
    Env(String),
    /// A dotenv variable.
    Dotenv {
        /// Variable name, e.g. `DB_PORT`.
        name: String,
        /// Definition site, e.g. `/abs/.env:3`.
        location: String,
    },
    /// A config-file value at `path:line:column`.
    File(String),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Flag(flag) => f.write_str(flag),
            Self::Env(name) => f.write_str(name),
            Self::Dotenv { name, location } => write!(f, "{name} in {location}"),
            Self::File(location) => f.write_str(location),
        }
    }
}

/// CLI flag and env var that set one config path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// Flag, e.g. `--db-name`.
    pub flag: String,
    /// Env var, e.g. `DB_NAME`, when the flag has one.
    pub env: Option<String>,
}

/// Sources and routes of every config path.
#[derive(Clone, Debug, Default)]
pub struct Provenance {
    sources: HashMap<String, Source>,
    routes: HashMap<String, Route>,
}

impl Provenance {
    /// Records `source` for `path` unless a higher-precedence source was recorded first.
    pub fn set_source(&mut self, path: &str, source: Source) {
        self.sources.entry(path.to_owned()).or_insert(source);
    }

    /// Records the CLI route for `path`, keeping the first one recorded.
    pub fn add_route(&mut self, path: &str, route: Route) {
        self.routes.entry(path.to_owned()).or_insert(route);
    }

    /// Returns where the value of `path` came from; `None` means the built-in default.
    #[must_use]
    pub fn source(&self, path: &str) -> Option<&Source> {
        self.sources.get(path)
    }

    /// Returns the note appended to `error`.
    ///
    /// A single-setting error names its source, or explains how to set it
    /// when it was never set. A cross-setting error lists the source of
    /// each involved setting that was set.
    #[must_use]
    pub fn note(&self, error: &ConfigError) -> String {
        match error.fields().as_slice() {
            [field] => match self.source(field) {
                Some(source) => format!(" (from {source})"),
                None => format!("; {}", self.hint(field)),
            },
            fields => {
                let set: Vec<String> = fields
                    .iter()
                    .filter_map(|field| self.source(field).map(|source| format!("{field} from {source}")))
                    .collect();
                if set.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", set.join("; "))
                }
            }
        }
    }

    /// Returns how to set `path`, e.g. `set --db-name, DB_NAME, or database.name in the config file`.
    fn hint(&self, path: &str) -> String {
        match self.routes.get(path) {
            Some(Route { flag, env: Some(env) }) => format!("set {flag}, {env}, or {path} in the config file"),
            Some(Route { flag, env: None }) => format!("set {flag} or {path} in the config file"),
            None => format!("set {path} in the config file"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> Provenance {
        let mut provenance = Provenance::default();
        provenance.add_route(
            "database.name",
            Route {
                flag: "--db-name".into(),
                env: Some("DB_NAME".into()),
            },
        );
        provenance
    }

    #[test]
    fn source_display() {
        assert_eq!(Source::Flag("--db-port".into()).to_string(), "--db-port");
        assert_eq!(Source::Env("DB_PORT".into()).to_string(), "DB_PORT");
        assert_eq!(
            Source::Dotenv {
                name: "DB_PORT".into(),
                location: "/abs/.env:3".into()
            }
            .to_string(),
            "DB_PORT in /abs/.env:3"
        );
        assert_eq!(
            Source::File("/abs/dbmcp.yaml:12:5".into()).to_string(),
            "/abs/dbmcp.yaml:12:5"
        );
    }

    #[test]
    fn first_source_wins() {
        let mut provenance = Provenance::default();
        provenance.set_source("database.port", Source::Flag("--db-port".into()));
        provenance.set_source("database.port", Source::File("/f:1:1".into()));
        assert_eq!(
            provenance.source("database.port"),
            Some(&Source::Flag("--db-port".into()))
        );
    }

    #[test]
    fn unset_single_field_gets_route_hint() {
        assert_eq!(
            provenance().note(&ConfigError::MissingSqliteDbName),
            "; set --db-name, DB_NAME, or database.name in the config file"
        );
        assert_eq!(
            provenance().note(&ConfigError::MissingSshUser),
            "; set ssh.user in the config file"
        );
    }

    #[test]
    fn set_single_field_names_its_source() {
        let mut provenance = provenance();
        provenance.set_source("database.name", Source::Env("DB_NAME".into()));
        assert_eq!(provenance.note(&ConfigError::MissingSqliteDbName), " (from DB_NAME)");
    }

    #[test]
    fn cross_field_lists_only_set_fields() {
        let mut provenance = provenance();
        assert_eq!(provenance.note(&ConfigError::SshSqliteUnsupported), "");
        provenance.set_source("ssh.host", Source::File("/abs/dbmcp.yaml:20:9".into()));
        provenance.set_source("database.backend", Source::Flag("--db-backend".into()));
        assert_eq!(
            provenance.note(&ConfigError::SshSqliteUnsupported),
            " (ssh.host from /abs/dbmcp.yaml:20:9; database.backend from --db-backend)"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one case per ConfigError variant")]
    fn every_variant_renders_subject_and_note() {
        let mut provenance = Provenance::default();
        for path in [
            "database.name",
            "database.ssl_ca",
            "database.backend",
            "database.page_size",
            "http.host",
            "http.auth_token",
            "http.allowed_origins",
            "pii.categories",
            "pii.ner_enabled",
            "ssh.host",
            "ssh.port",
            "ssh.key_passphrase",
        ] {
            provenance.set_source(path, Source::Flag(format!("--{path}")));
        }
        let render = |error: ConfigError| format!("{error}{}", provenance.note(&error));
        let cases = [
            (
                ConfigError::MissingSqliteDbName,
                "database.name: required (file path) for the sqlite backend (from --database.name)",
            ),
            (
                ConfigError::FileNotFound {
                    field: "database.ssl_ca",
                    path: "/ca.pem".into(),
                },
                "database.ssl_ca: file not found: /ca.pem (from --database.ssl_ca)",
            ),
            (
                ConfigError::EmptyHttpHost,
                "http.host: must not be empty (from --http.host)",
            ),
            (
                ConfigError::EmptyHttpAuthToken,
                "http.auth_token: must not be empty when set (from --http.auth_token)",
            ),
            (
                ConfigError::UnauthenticatedNonLoopbackBind("0.0.0.0".into()),
                "http.host: refusing to bind non-loopback host '0.0.0.0' without authentication; set http.auth_token or http.allow_unauthenticated (http.host from --http.host; http.auth_token from --http.auth_token)",
            ),
            (
                ConfigError::InvalidAllowedOrigin("x".into()),
                "http.allowed_origins: entry 'x' is not a valid origin; expected `<scheme>://<host>[:<port>|:*]` or `null` (from --http.allowed_origins)",
            ),
            (
                ConfigError::PiiCategoriesEmpty,
                "pii.categories: must not be empty when set; remove it to fall back to defaults (from --pii.categories)",
            ),
            (
                ConfigError::PiiNerModelMissing,
                "pii.ner_model: required (model directory) when pii.ner_enabled is set (pii.ner_enabled from --pii.ner_enabled)",
            ),
            (
                ConfigError::SshSqliteUnsupported,
                "ssh.host: not supported with the sqlite backend (ssh.host from --ssh.host; database.backend from --database.backend)",
            ),
            (
                ConfigError::MissingSshHost,
                "ssh.host: required when other ssh options are set (ssh.host from --ssh.host; ssh.port from --ssh.port; ssh.key_passphrase from --ssh.key_passphrase)",
            ),
            (
                ConfigError::EmptySshHost,
                "ssh.host: must not be empty (from --ssh.host)",
            ),
            (
                ConfigError::MissingSshUser,
                "ssh.user: required when ssh.host is set; set ssh.user in the config file",
            ),
            (
                ConfigError::MissingSshAuth,
                "ssh.password: required when ssh.host is set and ssh.key is not; set ssh.password in the config file",
            ),
            (
                ConfigError::SshKeyPassphraseWithoutKey,
                "ssh.key_passphrase: requires ssh.key (ssh.key_passphrase from --ssh.key_passphrase)",
            ),
            (
                ConfigError::MissingSshKnownHosts,
                "ssh.known_hosts: required when the home directory cannot be determined; set ssh.known_hosts in the config file",
            ),
            (
                ConfigError::BelowMinimum {
                    field: "ssh.port",
                    min: 1,
                },
                "ssh.port: must be at least 1 (from --ssh.port)",
            ),
            (
                ConfigError::OutOfRange {
                    field: "database.page_size",
                    min: 1,
                    max: 500,
                },
                "database.page_size: must be between 1 and 500 (from --database.page_size)",
            ),
            (
                ConfigError::EmptyListEntry("http.allowed_origins"),
                "http.allowed_origins: must not contain empty entries (from --http.allowed_origins)",
            ),
            (
                ConfigError::DuplicateListEntry {
                    field: "pii.categories",
                    entry: "contact".into(),
                },
                "pii.categories: contains duplicate entry 'contact' (from --pii.categories)",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(render(error), expected);
        }
    }

    #[test]
    fn every_variant_subject_is_its_first_field() {
        for error in [
            ConfigError::MissingSqliteDbName,
            ConfigError::EmptyHttpHost,
            ConfigError::UnauthenticatedNonLoopbackBind("h".into()),
            ConfigError::PiiNerModelMissing,
            ConfigError::SshSqliteUnsupported,
            ConfigError::MissingSshHost,
            ConfigError::MissingSshAuth,
            ConfigError::SshKeyPassphraseWithoutKey,
        ] {
            let subject = error.fields()[0];
            assert!(error.to_string().starts_with(&format!("{subject}: ")), "{error}");
        }
    }
}
