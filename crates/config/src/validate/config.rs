//! Validation rules for the composed config and the rules spanning sections.

use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};
use dbmcp_types::{Config, database};

/// Validates every section and the rules spanning sections.
///
/// Rules enforced, besides each present section's own rules:
/// - an SSH tunnel is not supported with the `SQLite` backend.
///
/// # Errors
///
/// Returns one [`Diagnostic`] per failing rule.
pub(crate) fn config(config: &Config) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    errors.extend(super::database(&config.database).err().into_iter().flatten());
    if let Some(section) = &config.http {
        errors.extend(super::http(section).err().into_iter().flatten());
    }
    if let Some(section) = &config.ssh {
        if config.database.backend == database::Backend::Sqlite {
            errors
                .push(Diagnostic::new(ConfigError::SshSqliteUnsupported).involving(&[env::SSH_HOST, env::DB_BACKEND]));
        }
        errors.extend(super::ssh(section).err().into_iter().flatten());
    }
    into_result(errors)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::validate::database::tests::db_config;
    use crate::validate::http::tests::config_with_host;
    use crate::validate::ssh::tests::ssh_config;
    use dbmcp_types::{Transport, http, privacy, ssh};

    fn mysql_config() -> Config {
        Config {
            database: database::Config {
                port: 3306,
                user: "root".into(),
                password: Some("secret".into()),
                ..database::Config::default()
            },
            transport: Transport::Stdio,
            http: None,
            privacy: None,
            ssh: None,
        }
    }

    fn sqlite() -> database::Config {
        database::Config {
            name: Some("x.db".into()),
            ..db_config(database::Backend::Sqlite)
        }
    }

    #[test]
    fn valid_config_passes() {
        let config = Config {
            ssh: Some(ssh_config()),
            ..mysql_config()
        };
        super::config(&config).expect("valid config");
    }

    #[test]
    fn ssh_on_sqlite_is_rejected() {
        let config = Config {
            database: sqlite(),
            ssh: Some(ssh_config()),
            ..mysql_config()
        };
        let errors = super::config(&config).expect_err("ssh on sqlite must fail");
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
        super::config(&config).expect("sqlite without ssh");
    }

    #[test]
    fn database_rules_run() {
        let config = Config {
            database: database::Config { name: None, ..sqlite() },
            ..mysql_config()
        };
        let errors = super::config(&config).expect_err("sqlite without name must fail");
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
        let errors = super::config(&config).expect_err("empty http host must fail");
        assert!(matches!(errors[0].error, ConfigError::EmptyHttpHost), "{errors:?}");
    }

    #[test]
    fn ssh_rules_run_when_present() {
        let config = Config {
            ssh: Some(ssh::Config {
                user: String::new(),
                ..ssh_config()
            }),
            ..mysql_config()
        };
        let errors = super::config(&config).expect_err("ssh without user must fail");
        assert!(matches!(errors[0].error, ConfigError::MissingSshUser), "{errors:?}");
    }

    #[test]
    fn config_debug_includes_privacy_section() {
        let config = Config {
            privacy: Some(privacy::Config {
                operator: privacy::Operator::Mask,
                categories: None,
                ..privacy::Config::default()
            }),
            ..mysql_config()
        };
        let debug = format!("{config:?}");
        let privacy = debug.split_once("privacy: Some(").map(|(_, rest)| rest);
        assert!(
            privacy.is_some_and(|rest| rest.contains("mode: Full, operator: Mask")),
            "expected privacy section in debug output: {debug}"
        );
    }

    #[test]
    fn config_debug_redacts_every_secret() {
        let config = Config {
            transport: Transport::Http,
            database: database::Config {
                password: Some("hunter2-db-password".into()),
                ..database::Config::default()
            },
            http: Some(http::Config {
                host: http::Config::DEFAULT_HOST.into(),
                port: http::Config::DEFAULT_PORT,
                allowed_origins: http::Config::default_allowed_origins(),
                allowed_hosts: http::Config::default_allowed_hosts(),
                auth_token: Some("hunter2-token".into()),
                allow_unauthenticated: false,
            }),
            ssh: Some(ssh::Config {
                host: "bastion".into(),
                port: ssh::Config::DEFAULT_PORT,
                user: "deploy".into(),
                password: Some("hunter2-ssh-password".into()),
                key: None,
                key_passphrase: Some("hunter2-passphrase".into()),
                known_hosts: None,
                strict_host_key_checking: ssh::StrictHostKeyChecking::Yes,
                keepalive_interval: Duration::ZERO,
                local_port: None,
            }),
            privacy: None,
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"), "secret leaked: {debug}");
        assert_eq!(debug.matches("[REDACTED]").count(), 4, "{debug}");
    }
}
