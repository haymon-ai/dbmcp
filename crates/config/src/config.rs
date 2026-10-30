//! Top-level [`Config`] composing the transport and the database, http, pii, and ssh sections.

use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};
use crate::http::HttpConfig;
use crate::pii::PiiConfig;
use crate::ssh::SshConfig;
use crate::transport::Transport;

/// Runtime configuration for the MCP server.
///
/// Composes the selected [`Transport`] with [`DatabaseConfig`] and the
/// optional [`HttpConfig`], [`PiiConfig`], and [`SshConfig`] sections.
/// HTTP config is present only when `transport` / `DBMCP_TRANSPORT` is `http`;
/// PII config only when `pii.enabled` / `DBMCP_PII_ENABLE` is on; SSH
/// config only when `ssh.host` / `DBMCP_SSH_HOST` is set. The log level is
/// returned next to it in [`Loaded`](crate::Loaded), not part of this struct.
#[derive(Clone, Debug)]
pub struct Config {
    /// MCP transport the server runs on.
    pub transport: Transport,

    /// Database connection and behavior settings.
    pub database: DatabaseConfig,

    /// HTTP transport settings (present only when the HTTP transport is selected).
    pub http: Option<HttpConfig>,

    /// PII redaction settings (`None` = redaction off).
    pub pii: Option<PiiConfig>,

    /// SSH tunnel settings (`None` = connect to the database directly).
    pub ssh: Option<SshConfig>,
}

impl Config {
    /// Validates every section and the rules spanning sections.
    ///
    /// Rules enforced, besides each present section's own `validate()`:
    /// - an SSH tunnel is not supported with the `SQLite` backend.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per failing rule.
    pub(crate) fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        errors.extend(self.database.validate().err().into_iter().flatten());
        if let Some(http) = &self.http {
            errors.extend(http.validate().err().into_iter().flatten());
        }
        if let Some(pii) = &self.pii {
            errors.extend(pii.validate().err().into_iter().flatten());
        }
        if let Some(ssh) = &self.ssh {
            if self.database.backend == DatabaseBackend::Sqlite {
                errors.push(
                    Diagnostic::new(ConfigError::SshSqliteUnsupported).involving(&[env::SSH_HOST, env::DB_BACKEND]),
                );
            }
            errors.extend(ssh.validate().err().into_iter().flatten());
        }
        into_result(errors)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::database::tests::db_config;
    use crate::http::tests::config_with_host;
    use crate::pii::PiiOperator;
    use crate::ssh::StrictHostKeyChecking;
    use crate::ssh::tests::ssh_config;

    fn mysql_config() -> Config {
        Config {
            database: DatabaseConfig {
                port: 3306,
                user: "root".into(),
                password: Some("secret".into()),
                ..DatabaseConfig::default()
            },
            transport: Transport::Stdio,
            http: None,
            pii: None,
            ssh: None,
        }
    }

    fn sqlite() -> DatabaseConfig {
        DatabaseConfig {
            name: Some("x.db".into()),
            ..db_config(DatabaseBackend::Sqlite)
        }
    }

    #[test]
    fn valid_config_passes() {
        let config = Config {
            ssh: Some(ssh_config()),
            ..mysql_config()
        };
        config.validate().expect("valid config");
    }

    #[test]
    fn ssh_on_sqlite_is_rejected() {
        let config = Config {
            database: sqlite(),
            ssh: Some(ssh_config()),
            ..mysql_config()
        };
        let errors = config.validate().expect_err("ssh on sqlite must fail");
        let [diagnostic] = errors.as_slice() else {
            panic!("expected one diagnostic: {errors:?}");
        };
        assert!(matches!(diagnostic.error, ConfigError::SshSqliteUnsupported));
        let involved: Vec<&str> = diagnostic.involved.iter().map(|(setting, _)| setting.path).collect();
        assert_eq!(involved, ["ssh.host", "database.backend"]);
    }

    #[test]
    fn sqlite_without_ssh_passes() {
        let config = Config {
            database: sqlite(),
            ..mysql_config()
        };
        config.validate().expect("sqlite without ssh");
    }

    #[test]
    fn database_rules_run() {
        let config = Config {
            database: DatabaseConfig { name: None, ..sqlite() },
            ..mysql_config()
        };
        let errors = config.validate().expect_err("sqlite without name must fail");
        assert!(
            matches!(errors[0].error, ConfigError::MissingSqliteDbName),
            "{errors:?}"
        );
    }

    #[test]
    fn http_rules_run_when_present() {
        let config = Config {
            transport: Transport::Http,
            http: Some(config_with_host(" ")),
            ..mysql_config()
        };
        let errors = config.validate().expect_err("empty http host must fail");
        assert!(matches!(errors[0].error, ConfigError::EmptyHttpHost), "{errors:?}");
    }

    #[test]
    fn pii_rules_run_when_present() {
        let config = Config {
            pii: Some(PiiConfig {
                ner_enabled: true,
                ner_model: None,
                ..PiiConfig::default()
            }),
            ..mysql_config()
        };
        let errors = config.validate().expect_err("ner without model must fail");
        assert!(matches!(errors[0].error, ConfigError::PiiNerModelMissing), "{errors:?}");
    }

    #[test]
    fn ssh_rules_run_when_present() {
        let config = Config {
            ssh: Some(SshConfig {
                user: String::new(),
                ..ssh_config()
            }),
            ..mysql_config()
        };
        let errors = config.validate().expect_err("ssh without user must fail");
        assert!(matches!(errors[0].error, ConfigError::MissingSshUser), "{errors:?}");
    }

    #[test]
    fn config_debug_includes_pii_section() {
        let config = Config {
            pii: Some(PiiConfig {
                operator: PiiOperator::Mask,
                categories: None,
                ..PiiConfig::default()
            }),
            ..mysql_config()
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("pii: Some(PiiConfig { operator: Mask"),
            "expected pii section in debug output: {debug}"
        );
    }

    #[test]
    fn config_debug_redacts_every_secret() {
        let config = Config {
            transport: Transport::Http,
            database: DatabaseConfig {
                password: Some("hunter2-db-password".into()),
                ..DatabaseConfig::default()
            },
            http: Some(HttpConfig {
                host: HttpConfig::DEFAULT_HOST.into(),
                port: HttpConfig::DEFAULT_PORT,
                allowed_origins: HttpConfig::default_allowed_origins(),
                allowed_hosts: HttpConfig::default_allowed_hosts(),
                auth_token: Some("hunter2-token".into()),
                allow_unauthenticated: false,
            }),
            ssh: Some(SshConfig {
                host: "bastion".into(),
                port: SshConfig::DEFAULT_PORT,
                user: "deploy".into(),
                password: Some("hunter2-ssh-password".into()),
                key: None,
                key_passphrase: Some("hunter2-passphrase".into()),
                known_hosts: None,
                strict_host_key_checking: StrictHostKeyChecking::Yes,
                keepalive_interval: Duration::ZERO,
                local_port: None,
            }),
            pii: None,
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"), "secret leaked: {debug}");
        assert_eq!(debug.matches("[REDACTED]").count(), 4, "{debug}");
    }
}
