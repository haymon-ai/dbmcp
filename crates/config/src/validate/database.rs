//! Validation rules for the `database` section.

use dbmcp_types::database;

use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};

/// Validates this configuration, accumulating every rule violation.
///
/// Rules enforced:
/// - `backend == Sqlite` requires `name` to be `Some(non-empty)`.
/// - `ssl == true` requires every set `ssl_ca` / `ssl_cert` / `ssl_key` path to exist.
///
/// # Errors
///
/// Returns one [`Diagnostic`] per failing rule.
pub(crate) fn database(config: &database::Config) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();

    if config.backend == database::Backend::Sqlite && config.name.as_deref().unwrap_or_default().is_empty() {
        errors.push(
            Diagnostic::new(ConfigError::MissingSqliteDbName)
                .involving(&[env::DB_BACKEND])
                .missing(&[env::DB_NAME]),
        );
    }

    if config.max_query_timeout > 0 {
        let involving = &[env::DB_QUERY_TIMEOUT, env::DB_MAX_QUERY_TIMEOUT];
        if config.query_timeout == 0 {
            errors.push(Diagnostic::new(ConfigError::MaxQueryTimeoutWithoutTimeout).involving(involving));
        } else if config.max_query_timeout < config.query_timeout {
            errors.push(
                Diagnostic::new(ConfigError::MaxQueryTimeoutBelowTimeout(
                    config.max_query_timeout,
                    config.query_timeout,
                ))
                .involving(involving),
            );
        }
    }

    if config.ssl {
        for (setting, path) in [
            (env::DB_SSL_CA, &config.ssl_ca),
            (env::DB_SSL_CERT, &config.ssl_cert),
            (env::DB_SSL_KEY, &config.ssl_key),
        ] {
            if let Some(path) = path
                && !std::path::Path::new(path).exists()
            {
                errors
                    .push(Diagnostic::new(ConfigError::FileNotFound(setting.path, path.clone())).involving(&[setting]));
            }
        }
    }

    into_result(errors)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dbmcp_types::{Config, Transport};

    pub(crate) fn db_config(backend: database::Backend) -> database::Config {
        database::Config {
            backend,
            port: backend.default_port(),
            user: backend.default_user().into(),
            ..database::Config::default()
        }
    }

    fn base_config(backend: database::Backend) -> Config {
        Config {
            database: db_config(backend),
            transport: Transport::Stdio,
            http: None,
            privacy: None,
            ssh: None,
        }
    }

    fn mysql_config() -> Config {
        Config {
            database: database::Config {
                port: 3306,
                user: "root".into(),
                password: Some("secret".into()),
                ..db_config(database::Backend::Mysql)
            },
            ..base_config(database::Backend::Mysql)
        }
    }

    #[test]
    fn defaults_resolved_at_construction() {
        let mysql = base_config(database::Backend::Mysql);
        assert_eq!(mysql.database.host, "localhost");
        assert_eq!(mysql.database.port, 3306);
        assert_eq!(mysql.database.user, "root");

        let pg = base_config(database::Backend::Postgres);
        assert_eq!(pg.database.port, 5432);
        assert_eq!(pg.database.user, "postgres");

        let sqlite = base_config(database::Backend::Sqlite);
        assert_eq!(sqlite.database.port, 0);
        assert_eq!(sqlite.database.user, "");
    }

    #[test]
    fn explicit_values_override_defaults() {
        let config = Config {
            database: database::Config {
                host: "dbserver.example.com".into(),
                port: 13306,
                user: "myuser".into(),
                ..db_config(database::Backend::Mysql)
            },
            ..base_config(database::Backend::Mysql)
        };
        assert_eq!(config.database.host, "dbserver.example.com");
        assert_eq!(config.database.port, 13306);
        assert_eq!(config.database.user, "myuser");
    }

    #[test]
    fn mysql_without_user_gets_default() {
        let config = base_config(database::Backend::Mysql);
        assert_eq!(config.database.user, "root");
    }

    #[test]
    fn mariadb_backend_default_user_is_root() {
        let config = base_config(database::Backend::Mariadb);
        assert_eq!(config.database.user, "root");
        assert_eq!(config.database.port, 3306);
    }

    #[test]
    fn page_size_default_is_100() {
        let config = database::Config::default();
        assert_eq!(config.page_size, 100);
    }

    #[test]
    fn character_caps_default_to_4096_and_32000() {
        let config = database::Config::default();
        assert_eq!(config.max_cell_chars, 4096);
        assert_eq!(config.max_response_chars, 32_000);
    }

    #[test]
    fn debug_includes_page_size() {
        let config = database::Config {
            page_size: 250,
            ..mysql_config().database
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("page_size: 250"),
            "expected page_size in debug output: {debug}"
        );
    }

    #[test]
    fn validate_checks_max_query_timeout_against_query_timeout() {
        let config = |query_timeout, max_query_timeout| database::Config {
            query_timeout,
            max_query_timeout,
            ..database::Config::default()
        };
        for (timeout, max) in [(30, 30), (30, 120), (30, 0), (0, 0)] {
            assert!(super::database(&config(timeout, max)).is_ok(), "{timeout}/{max}");
        }

        let errors = super::database(&config(0, 60)).expect_err("cap without a timeout");
        assert!(matches!(errors[0].error, ConfigError::MaxQueryTimeoutWithoutTimeout));
        let errors = super::database(&config(30, 10)).expect_err("cap below the timeout");
        assert!(matches!(
            errors[0].error,
            ConfigError::MaxQueryTimeoutBelowTimeout(10, 30)
        ));
    }

    #[test]
    fn validate_rejects_sqlite_with_none_name() {
        let config = database::Config {
            backend: database::Backend::Sqlite,
            name: None,
            ..database::Config::default()
        };
        let errors = super::database(&config).expect_err("sqlite without name must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::MissingSqliteDbName))
        );
    }

    #[test]
    fn validate_rejects_sqlite_with_empty_name() {
        let config = database::Config {
            backend: database::Backend::Sqlite,
            name: Some(String::new()),
            ..database::Config::default()
        };
        let errors = super::database(&config).expect_err("empty sqlite name must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::MissingSqliteDbName))
        );
    }

    #[test]
    fn validate_rejects_missing_ssl_ca_path() {
        let config = database::Config {
            ssl: true,
            ssl_ca: Some("/nonexistent/ca.pem".into()),
            ..database::Config::default()
        };
        let errors = super::database(&config).expect_err("missing ssl ca must fail");
        assert!(errors.iter().any(|e| matches!(
            &e.error,
            ConfigError::FileNotFound(name, path)
                if *name == "database.ssl_ca" && path == "/nonexistent/ca.pem"
        )));
    }

    #[test]
    fn validate_collects_all_three_missing_ssl_paths_in_ca_cert_key_order() {
        let config = database::Config {
            ssl: true,
            ssl_ca: Some("/nope/ca.pem".into()),
            ssl_cert: Some("/nope/cert.pem".into()),
            ssl_key: Some("/nope/key.pem".into()),
            ..database::Config::default()
        };
        let errors = super::database(&config).expect_err("three missing files must fail");
        assert_eq!(errors.len(), 3);
        assert!(matches!(&errors[0].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_ca"));
        assert!(matches!(&errors[1].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_cert"));
        assert!(matches!(&errors[2].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_key"));
    }

    #[test]
    fn validate_skips_ssl_checks_when_ssl_disabled() {
        let config = database::Config {
            ssl: false,
            ssl_ca: Some("/nonexistent/ca.pem".into()),
            ..database::Config::default()
        };
        super::database(&config).expect("ssl off must skip cert checks");
    }

    #[test]
    fn validate_accumulates_sqlite_name_and_ssl_errors() {
        let config = database::Config {
            backend: database::Backend::Sqlite,
            name: None,
            ssl: true,
            ssl_ca: Some("/nope/ca.pem".into()),
            ..database::Config::default()
        };
        let errors = super::database(&config).expect_err("must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::MissingSqliteDbName))
        );
        assert!(errors.iter().any(|e| matches!(
            &e.error,
            ConfigError::FileNotFound(name, _) if *name == "database.ssl_ca"
        )));
    }

    #[test]
    fn debug_includes_query_timeout() {
        let config = Config {
            database: database::Config {
                query_timeout: 30,
                ..db_config(database::Backend::Mysql)
            },
            ..base_config(database::Backend::Mysql)
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("query_timeout: 30"),
            "expected query_timeout in debug output: {debug}"
        );
    }
}
