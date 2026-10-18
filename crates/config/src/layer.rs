//! Partial configuration layers and their resolution into a [`Config`].
//!
//! A [`ConfigLayer`] mirrors [`Config`] plus the log level, with every
//! field optional. The same type is the YAML config-file schema and the
//! CLI's "explicitly set" layer. [`ConfigLayer::or`] stacks two layers,
//! and [`ConfigLayer::resolve`] fills the remaining gaps from the
//! `DEFAULT_*` constants and validates the result once.
//!
//! File keys are the `snake_case` field names below; unknown keys are
//! rejected at every level. A `null` value, or a bare section, is the same
//! as leaving the key out. Lists replace lower layers wholesale.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::Config;
use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::error::{ConfigError, ConfigErrors};
use crate::http::HttpConfig;
use crate::log_level::LogLevel;
use crate::pii::{PiiCategory, PiiConfig, PiiOperator};
use crate::ssh::{self, SshConfig, StrictHostKeyChecking};
use secrecy::SecretString;

/// Transport whose sections [`ConfigLayer::resolve`] builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Stdio transport; the `http` section is ignored.
    Stdio,
    /// Streamable HTTP transport.
    Http,
}

/// Fully resolved settings produced by [`ConfigLayer::resolve`].
#[derive(Debug)]
pub struct Resolved {
    /// Validated server configuration.
    pub config: Config,
    /// Log level for the tracing subscriber.
    pub log_level: LogLevel,
}

/// Partial configuration: every setting is optional.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigLayer {
    /// Log level.
    pub log_level: Option<LogLevel>,
    /// Database settings.
    #[serde(default)]
    pub database: DatabaseLayer,
    /// HTTP transport settings.
    #[serde(default)]
    pub http: HttpLayer,
    /// PII redaction settings.
    #[serde(default)]
    pub pii: PiiLayer,
    /// SSH tunnel settings.
    #[serde(default)]
    pub ssh: SshLayer,
}

/// Partial [`DatabaseConfig`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseLayer {
    /// Database backend.
    pub backend: Option<DatabaseBackend>,
    /// Database host.
    pub host: Option<String>,
    /// Database port.
    pub port: Option<u16>,
    /// Database user.
    pub user: Option<String>,
    /// Database password.
    pub password: Option<SecretString>,
    /// Database name or `SQLite` file path.
    pub name: Option<String>,
    /// Character set (MySQL/MariaDB only).
    pub charset: Option<String>,
    /// Enable SSL/TLS.
    pub ssl: Option<bool>,
    /// Path to the CA certificate.
    pub ssl_ca: Option<String>,
    /// Path to the client certificate.
    pub ssl_cert: Option<String>,
    /// Path to the client key.
    pub ssl_key: Option<String>,
    /// Verify the server certificate.
    pub ssl_verify_cert: Option<bool>,
    /// Read-only mode.
    pub read_only: Option<bool>,
    /// Maximum connection pool size.
    pub max_pool_size: Option<u32>,
    /// Connection timeout in seconds.
    pub connection_timeout: Option<u64>,
    /// Query timeout in seconds (`0` disables it).
    pub query_timeout: Option<u64>,
    /// Maximum items per paginated tool response.
    pub page_size: Option<u16>,
}

/// Partial [`HttpConfig`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpLayer {
    /// Bind host.
    pub host: Option<String>,
    /// Bind port.
    pub port: Option<u16>,
    /// Allowed browser origins.
    pub allowed_origins: Option<Vec<String>>,
    /// Allowed host names.
    pub allowed_hosts: Option<Vec<String>>,
    /// Bearer token required on every request.
    pub auth_token: Option<SecretString>,
    /// Permit a non-loopback bind without a token.
    pub allow_unauthenticated: Option<bool>,
}

/// Partial [`PiiConfig`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiiLayer {
    /// Enable PII redaction.
    pub enabled: Option<bool>,
    /// Operator applied to detected spans.
    pub operator: Option<PiiOperator>,
    /// Categories the analyzer covers.
    pub categories: Option<Vec<PiiCategory>>,
    /// Enable the ML/NER pass.
    pub ner_enabled: Option<bool>,
    /// Path to the NER model directory.
    pub ner_model: Option<PathBuf>,
}

/// Partial [`SshConfig`]; setting `host` enables the tunnel.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshLayer {
    /// SSH server host; enables the tunnel.
    pub host: Option<String>,
    /// SSH server port.
    pub port: Option<u16>,
    /// SSH user.
    pub user: Option<String>,
    /// SSH password.
    pub password: Option<SecretString>,
    /// Path to the SSH private key.
    pub key: Option<PathBuf>,
    /// Passphrase for the private key.
    pub key_passphrase: Option<SecretString>,
    /// Path to the `known_hosts` file.
    pub known_hosts: Option<PathBuf>,
    /// Host-key verification policy.
    pub strict_host_key_checking: Option<StrictHostKeyChecking>,
    /// Keepalive interval in seconds (`0` disables it).
    pub keepalive_interval: Option<u64>,
    /// Local tunnel port.
    pub local_port: Option<u16>,
}

/// Implements a field-wise `or` and `set_paths` for a layer section.
macro_rules! impl_or {
    ($ty:ident, $section:literal { $($field:ident),* $(,)? }) => {
        impl $ty {
            /// Config path of every field, in declaration order.
            pub const PATHS: &'static [&'static str] = &[$(concat!($section, ".", stringify!($field))),*];

            /// Returns `self`, with every unset field taken from `lower`.
            #[must_use]
            pub fn or(self, lower: Self) -> Self {
                Self { $($field: self.$field.or(lower.$field)),* }
            }

            /// Returns the config path of every set field.
            #[must_use]
            pub fn set_paths(&self) -> Vec<&'static str> {
                let mut paths = Vec::new();
                $(if self.$field.is_some() {
                    paths.push(concat!($section, ".", stringify!($field)));
                })*
                paths
            }
        }
    };
}

impl_or!(DatabaseLayer, "database" {
    backend,
    host,
    port,
    user,
    password,
    name,
    charset,
    ssl,
    ssl_ca,
    ssl_cert,
    ssl_key,
    ssl_verify_cert,
    read_only,
    max_pool_size,
    connection_timeout,
    query_timeout,
    page_size,
});
impl_or!(HttpLayer, "http" {
    host,
    port,
    allowed_origins,
    allowed_hosts,
    auth_token,
    allow_unauthenticated,
});
impl_or!(PiiLayer, "pii" {
    enabled,
    operator,
    categories,
    ner_enabled,
    ner_model,
});
impl_or!(SshLayer, "ssh" {
    host,
    port,
    user,
    password,
    key,
    key_passphrase,
    known_hosts,
    strict_host_key_checking,
    keepalive_interval,
    local_port,
});

impl ConfigLayer {
    /// Returns `self`, with every unset setting taken from `lower`.
    #[must_use]
    pub fn or(self, lower: Self) -> Self {
        Self {
            log_level: self.log_level.or(lower.log_level),
            database: self.database.or(lower.database),
            http: self.http.or(lower.http),
            pii: self.pii.or(lower.pii),
            ssh: self.ssh.or(lower.ssh),
        }
    }

    /// Returns the config path of every set setting, e.g. `database.port`.
    #[must_use]
    pub fn set_paths(&self) -> Vec<&'static str> {
        let mut paths = Vec::new();
        if self.log_level.is_some() {
            paths.push("log_level");
        }
        paths.extend(self.database.set_paths());
        paths.extend(self.http.set_paths());
        paths.extend(self.pii.set_paths());
        paths.extend(self.ssh.set_paths());
        paths
    }

    /// Applies defaults to every unset setting and validates the result.
    ///
    /// The `http` section is built only for [`Transport::Http`]. `home`
    /// expands a leading `~` in SSH paths and locates the default
    /// `known_hosts` file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] with every rule violation across the
    /// database, http, pii, and ssh sections, in that order.
    pub fn resolve(self, transport: Transport, home: Option<&Path>) -> Result<Resolved, ConfigErrors> {
        let database = self.database.resolve();
        let http = (transport == Transport::Http).then(|| self.http.resolve());
        let pii = self.pii.resolve();
        let ssh = self.ssh.resolve(home);

        let mut errors = Vec::new();
        if let Err(e) = database.validate() {
            errors.extend(e);
        }
        if let Some(Err(e)) = http.as_ref().map(HttpConfig::validate) {
            errors.extend(e);
        }
        if let Err(e) = pii.validate() {
            errors.extend(e);
        }
        let ssh = match ssh {
            Ok(ssh) => {
                if let Some(Err(e)) = ssh.as_ref().map(|ssh| ssh.validate(database.backend)) {
                    errors.extend(e);
                }
                ssh
            }
            Err(e) => {
                errors.push(e);
                None
            }
        };
        if let Some(errors) = ConfigErrors::from_vec(errors) {
            return Err(errors);
        }

        Ok(Resolved {
            config: Config {
                database,
                http,
                pii,
                ssh,
            },
            log_level: self.log_level.unwrap_or(LogLevel::DEFAULT),
        })
    }
}

impl DatabaseLayer {
    fn resolve(self) -> DatabaseConfig {
        let backend = self.backend.unwrap_or(DatabaseConfig::DEFAULT_BACKEND);
        DatabaseConfig {
            backend,
            host: self.host.unwrap_or_else(|| DatabaseConfig::DEFAULT_HOST.into()),
            port: self.port.unwrap_or_else(|| backend.default_port()),
            user: self.user.unwrap_or_else(|| backend.default_user().into()),
            password: self.password,
            name: self.name,
            charset: self.charset,
            ssl: self.ssl.unwrap_or(DatabaseConfig::DEFAULT_SSL),
            ssl_ca: self.ssl_ca,
            ssl_cert: self.ssl_cert,
            ssl_key: self.ssl_key,
            ssl_verify_cert: self.ssl_verify_cert.unwrap_or(DatabaseConfig::DEFAULT_SSL_VERIFY_CERT),
            read_only: self.read_only.unwrap_or(DatabaseConfig::DEFAULT_READ_ONLY),
            max_pool_size: self.max_pool_size.unwrap_or(DatabaseConfig::DEFAULT_MAX_POOL_SIZE),
            connection_timeout: self.connection_timeout,
            query_timeout: self.query_timeout.unwrap_or(DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS),
            page_size: self.page_size.unwrap_or(DatabaseConfig::DEFAULT_PAGE_SIZE),
        }
    }
}

impl HttpLayer {
    fn resolve(self) -> HttpConfig {
        HttpConfig {
            host: self.host.unwrap_or_else(|| HttpConfig::DEFAULT_HOST.into()),
            port: self.port.unwrap_or(HttpConfig::DEFAULT_PORT),
            allowed_origins: self.allowed_origins.unwrap_or_else(HttpConfig::default_allowed_origins),
            allowed_hosts: self.allowed_hosts.unwrap_or_else(HttpConfig::default_allowed_hosts),
            auth_token: self.auth_token,
            allow_unauthenticated: self.allow_unauthenticated.unwrap_or(false),
        }
    }
}

impl PiiLayer {
    fn resolve(self) -> PiiConfig {
        PiiConfig {
            enabled: self.enabled.unwrap_or(PiiConfig::DEFAULT_ENABLED),
            operator: self.operator.unwrap_or(PiiConfig::DEFAULT_OPERATOR),
            categories: self.categories,
            ner_enabled: self.ner_enabled.unwrap_or(PiiConfig::DEFAULT_NER_ENABLED),
            ner_model: self.ner_model,
        }
    }
}

impl SshLayer {
    /// Builds the tunnel settings; `Ok(None)` when no SSH option is set.
    fn resolve(self, home: Option<&Path>) -> Result<Option<SshConfig>, ConfigError> {
        let Some(host) = self.host.clone() else {
            return if self.any_set() {
                Err(ConfigError::MissingSshHost)
            } else {
                Ok(None)
            };
        };

        let known_hosts = match &self.known_hosts {
            Some(path) => Some(ssh::expand_tilde(path, home)),
            None => home.map(|home| home.join(".ssh").join("known_hosts")),
        };
        Ok(Some(SshConfig {
            host,
            port: self.port.unwrap_or(SshConfig::DEFAULT_PORT),
            user: self.user.unwrap_or_default(),
            password: self.password,
            key: self.key.as_deref().map(|key| ssh::expand_tilde(key, home)),
            key_passphrase: self.key_passphrase,
            known_hosts,
            strict_host_key_checking: self
                .strict_host_key_checking
                .unwrap_or(SshConfig::DEFAULT_STRICT_HOST_KEY_CHECKING),
            keepalive_interval: Duration::from_secs(
                self.keepalive_interval
                    .unwrap_or(SshConfig::DEFAULT_KEEPALIVE_INTERVAL_SECS),
            ),
            local_port: self.local_port,
        }))
    }

    /// Returns `true` when any option other than `host` is set.
    fn any_set(&self) -> bool {
        self.set_paths().iter().any(|path| *path != "ssh.host")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::home;
    use secrecy::ExposeSecret;

    fn resolve(layer: ConfigLayer, transport: Transport) -> Result<Resolved, ConfigErrors> {
        layer.resolve(transport, Some(&home()))
    }

    fn messages(errors: &ConfigErrors) -> Vec<String> {
        errors.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn empty_layer_resolves_to_defaults() {
        let resolved = resolve(ConfigLayer::default(), Transport::Http).expect("defaults validate");
        assert_eq!(resolved.log_level, LogLevel::Info);
        let config = resolved.config;
        let db = config.database;
        assert_eq!(db.backend, DatabaseBackend::Mysql);
        assert_eq!(db.host, "localhost");
        assert_eq!((db.port, db.user.as_str()), (3306, "root"));
        assert!(db.read_only);
        assert_eq!(db.max_pool_size, DatabaseConfig::DEFAULT_MAX_POOL_SIZE);
        assert_eq!(db.query_timeout, DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS);
        assert_eq!(db.page_size, DatabaseConfig::DEFAULT_PAGE_SIZE);
        let http = config.http.expect("http transport builds http");
        assert_eq!(http.host, HttpConfig::DEFAULT_HOST);
        assert_eq!(http.allowed_origins, HttpConfig::default_allowed_origins());
        assert!(!config.pii.enabled);
        assert!(config.ssh.is_none());
    }

    #[test]
    fn database_layer_merge_prefers_upper_then_lower_then_default() {
        let upper = DatabaseLayer {
            host: Some("upper".into()),
            read_only: Some(false),
            ..DatabaseLayer::default()
        };
        let lower = DatabaseLayer {
            backend: Some(DatabaseBackend::Postgres),
            host: Some("lower".into()),
            user: Some("app".into()),
            password: Some("pw".into()),
            read_only: Some(true),
            query_timeout: Some(0),
            ..DatabaseLayer::default()
        };
        let db = upper.or(lower).resolve();
        assert_eq!(db.backend, DatabaseBackend::Postgres);
        assert_eq!(db.host, "upper");
        assert_eq!(db.port, 5432, "port default follows merged backend");
        assert_eq!(db.user, "app");
        assert_eq!(db.password.as_ref().map(ExposeSecret::expose_secret), Some("pw"));
        assert!(!db.read_only);
        assert_eq!(db.query_timeout, 0);
        assert_eq!(db.page_size, DatabaseConfig::DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn http_layer_merge_replaces_lists_wholesale() {
        let upper = HttpLayer {
            allowed_origins: Some(vec!["https://app.example:*".into()]),
            ..HttpLayer::default()
        };
        let lower = HttpLayer {
            port: Some(9100),
            allowed_origins: Some(vec!["http://a.example:*".into(), "http://b.example:*".into()]),
            allowed_hosts: Some(vec!["db.example".into()]),
            ..HttpLayer::default()
        };
        let http = upper.or(lower).resolve();
        assert_eq!(http.port, 9100);
        assert_eq!(http.allowed_origins, ["https://app.example:*"]);
        assert_eq!(http.allowed_hosts, ["db.example"]);
        assert_eq!(http.host, HttpConfig::DEFAULT_HOST);
    }

    #[test]
    fn pii_layer_merge() {
        let upper = PiiLayer {
            operator: Some(PiiOperator::Hash),
            ..PiiLayer::default()
        };
        let lower = PiiLayer {
            enabled: Some(true),
            operator: Some(PiiOperator::Mask),
            categories: Some(vec![PiiCategory::Contact]),
            ..PiiLayer::default()
        };
        let pii = upper.or(lower).resolve();
        assert!(pii.enabled);
        assert_eq!(pii.operator, PiiOperator::Hash);
        assert_eq!(pii.categories, Some(vec![PiiCategory::Contact]));
        assert!(!pii.ner_enabled);
    }

    #[test]
    fn ssh_layer_merge_and_defaults() {
        let upper = SshLayer {
            port: Some(2222),
            ..SshLayer::default()
        };
        let lower = SshLayer {
            host: Some("bastion".into()),
            user: Some("deploy".into()),
            password: Some("pw".into()),
            port: Some(22),
            known_hosts: Some(PathBuf::from("~/.ssh/custom_hosts")),
            ..SshLayer::default()
        };
        let ssh = upper
            .or(lower)
            .resolve(Some(&home()))
            .expect("valid")
            .expect("host enables tunnel");
        assert_eq!(ssh.host, "bastion");
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.known_hosts, Some(home().join(".ssh").join("custom_hosts")));
        assert_eq!(ssh.strict_host_key_checking, StrictHostKeyChecking::Yes);
        assert_eq!(ssh.keepalive_interval, Duration::from_secs(30));
    }

    #[test]
    fn ssh_defaults_known_hosts_under_home() {
        let ssh = SshLayer {
            host: Some("bastion".into()),
            ..SshLayer::default()
        }
        .resolve(Some(&home()))
        .expect("valid")
        .expect("enabled");
        assert_eq!(ssh.known_hosts, Some(home().join(".ssh").join("known_hosts")));
        assert_eq!(ssh.port, SshConfig::DEFAULT_PORT);
    }

    #[test]
    fn ssh_options_without_host_are_rejected() {
        let layer = ConfigLayer {
            ssh: SshLayer {
                user: Some("deploy".into()),
                ..SshLayer::default()
            },
            ..ConfigLayer::default()
        };
        let errors = resolve(layer, Transport::Stdio).expect_err("orphan ssh option");
        assert_eq!(messages(&errors), ["ssh.host: required when other ssh options are set"]);
    }

    #[test]
    fn ssh_host_from_lower_layer_enables_tunnel_for_upper_options() {
        let upper = ConfigLayer {
            ssh: SshLayer {
                user: Some("deploy".into()),
                password: Some("pw".into()),
                ..SshLayer::default()
            },
            ..ConfigLayer::default()
        };
        let lower = ConfigLayer {
            ssh: SshLayer {
                host: Some("bastion".into()),
                ..SshLayer::default()
            },
            ..ConfigLayer::default()
        };
        let config = resolve(upper.or(lower), Transport::Stdio).expect("valid").config;
        assert_eq!(config.ssh.expect("enabled").user, "deploy");
    }

    #[test]
    fn http_section_is_ignored_under_stdio() {
        let layer = ConfigLayer {
            http: HttpLayer {
                host: Some(String::new()),
                ..HttpLayer::default()
            },
            ..ConfigLayer::default()
        };
        let config = resolve(layer.clone(), Transport::Stdio)
            .expect("stdio ignores http")
            .config;
        assert!(config.http.is_none());
        let errors = resolve(layer, Transport::Http).expect_err("http validates http");
        assert_eq!(messages(&errors), ["http.host: must not be empty"]);
    }

    #[test]
    fn errors_accumulate_in_section_order() {
        let layer = ConfigLayer {
            database: DatabaseLayer {
                backend: Some(DatabaseBackend::Sqlite),
                ..DatabaseLayer::default()
            },
            http: HttpLayer {
                host: Some(String::new()),
                ..HttpLayer::default()
            },
            pii: PiiLayer {
                ner_enabled: Some(true),
                ..PiiLayer::default()
            },
            ssh: SshLayer {
                port: Some(0),
                ..SshLayer::default()
            },
            ..ConfigLayer::default()
        };
        let errors = resolve(layer, Transport::Http).expect_err("must fail");
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(errors[1], ConfigError::EmptyHttpHost));
        assert!(matches!(errors[2], ConfigError::PiiNerModelMissing));
        assert!(matches!(errors[3], ConfigError::MissingSshHost));
        assert_eq!(errors.len(), 4);
    }

    #[test]
    fn ssh_validation_uses_merged_backend() {
        let layer = ConfigLayer {
            database: DatabaseLayer {
                backend: Some(DatabaseBackend::Sqlite),
                name: Some(":memory:".into()),
                ..DatabaseLayer::default()
            },
            ssh: SshLayer {
                host: Some("bastion".into()),
                user: Some("deploy".into()),
                password: Some("pw".into()),
                ..SshLayer::default()
            },
            ..ConfigLayer::default()
        };
        let errors = resolve(layer, Transport::Stdio).expect_err("sqlite cannot tunnel");
        assert!(matches!(errors[0], ConfigError::SshSqliteUnsupported));
    }

    #[test]
    fn set_paths_lists_config_paths_of_set_fields() {
        let mut layer = ConfigLayer {
            log_level: Some(LogLevel::Debug),
            ..ConfigLayer::default()
        };
        layer.database.port = Some(1);
        layer.http.allowed_hosts = Some(vec![]);
        layer.pii.ner_model = Some(PathBuf::from("/m"));
        layer.ssh.key_passphrase = Some("pp".into());
        assert_eq!(
            layer.set_paths(),
            [
                "log_level",
                "database.port",
                "http.allowed_hosts",
                "pii.ner_model",
                "ssh.key_passphrase"
            ]
        );
        assert!(ConfigLayer::default().set_paths().is_empty());
    }

    #[test]
    fn log_level_merges() {
        let upper = ConfigLayer {
            log_level: Some(LogLevel::Trace),
            ..ConfigLayer::default()
        };
        let lower = ConfigLayer {
            log_level: Some(LogLevel::Warn),
            ..ConfigLayer::default()
        };
        assert_eq!(upper.or(lower.clone()).log_level, Some(LogLevel::Trace));
        assert_eq!(ConfigLayer::default().or(lower).log_level, Some(LogLevel::Warn));
    }
}
