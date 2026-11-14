//! Database connection settings and backend enum.

use secrecy::SecretString;

use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};

/// Supported database backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// `MySQL` database.
    Mysql,
    /// `MariaDB` database (uses the `MySQL` driver).
    Mariadb,
    /// `PostgreSQL` database.
    Postgres,
    /// `SQLite` file-based database.
    Sqlite,
}

impl std::fmt::Display for DatabaseBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mysql => write!(f, "mysql"),
            Self::Mariadb => write!(f, "mariadb"),
            Self::Postgres => write!(f, "postgres"),
            Self::Sqlite => write!(f, "sqlite"),
        }
    }
}

impl DatabaseBackend {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[
        ("mysql", Self::Mysql),
        ("mariadb", Self::Mariadb),
        ("postgres", Self::Postgres),
        ("sqlite", Self::Sqlite),
    ];

    /// Returns the default port for this backend.
    #[must_use]
    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::Mysql | Self::Mariadb => 3306,
            Self::Sqlite => 0,
        }
    }

    /// Returns the default username for this backend.
    #[must_use]
    pub fn default_user(self) -> &'static str {
        match self {
            Self::Mysql | Self::Mariadb => "root",
            Self::Postgres => "postgres",
            Self::Sqlite => "",
        }
    }
}

/// Database connection and behavior settings.
///
/// All fields are fully resolved — no `Option` indirection for connection
/// fields. Defaults are applied by the loader.
#[derive(Clone, Debug)]
pub struct DatabaseConfig {
    /// Database backend type.
    pub backend: DatabaseBackend,

    /// Database host (resolved default: `"localhost"`).
    pub host: String,

    /// Database port (resolved default: backend-dependent).
    pub port: u16,

    /// Database user (resolved default: backend-dependent).
    pub user: String,

    /// Database password.
    pub password: Option<SecretString>,

    /// Database name or `SQLite` file path.
    pub name: Option<String>,

    /// Character set for MySQL/MariaDB connections.
    pub charset: Option<String>,

    /// Enable SSL/TLS for the database connection.
    pub ssl: bool,

    /// Path to the CA certificate for SSL.
    pub ssl_ca: Option<String>,

    /// Path to the client certificate for SSL.
    pub ssl_cert: Option<String>,

    /// Path to the client key for SSL.
    pub ssl_key: Option<String>,

    /// Whether to verify the server certificate.
    pub ssl_verify_cert: bool,

    /// Whether the server runs in read-only mode.
    pub read_only: bool,

    /// Maximum database connection pool size.
    pub max_pool_size: u32,

    /// Connection timeout in seconds (`None` = driver default).
    pub connection_timeout: Option<u64>,

    /// Query execution timeout in seconds.
    ///
    /// Defaults to [`Self::DEFAULT_QUERY_TIMEOUT_SECS`]. `0` disables dbmcp's
    /// server and client timeouts; server defaults apply.
    pub query_timeout: u64,

    /// Largest per-call `readQuery` timeout in seconds (`0` = no cap).
    ///
    /// Defaults to [`Self::query_timeout`], so callers can only shorten it.
    pub max_query_timeout: u64,

    /// Maximum items returned in a single paginated tool response.
    ///
    /// Applies uniformly to every paginated tool (currently `list_tables`).
    /// Range `1..=500`, enforced by the loader.
    pub page_size: u16,

    /// Maximum characters per string value in query tool rows.
    ///
    /// Longer values are cut and marked. `0` disables the cap.
    pub max_cell_chars: usize,

    /// Maximum characters of one query tool text result.
    ///
    /// Trailing rows past the budget are dropped: a paged `SELECT` resumes
    /// them via its cursor, other calls count them. `0` disables the cap.
    pub max_response_chars: usize,
}

impl DatabaseConfig {
    /// Default database backend.
    pub const DEFAULT_BACKEND: DatabaseBackend = DatabaseBackend::Mysql;
    /// Default database host.
    pub const DEFAULT_HOST: &'static str = "localhost";
    /// Default SSL enabled state.
    pub const DEFAULT_SSL: bool = false;
    /// Default SSL certificate verification.
    pub const DEFAULT_SSL_VERIFY_CERT: bool = true;
    /// Default read-only mode.
    pub const DEFAULT_READ_ONLY: bool = true;
    /// Default connection pool size.
    pub const DEFAULT_MAX_POOL_SIZE: u32 = 5;
    /// Default idle timeout in seconds (10 minutes).
    pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;
    /// Default max lifetime in seconds (30 minutes).
    pub const DEFAULT_MAX_LIFETIME_SECS: u64 = 1800;
    /// Default minimum connections in pool.
    pub const DEFAULT_MIN_CONNECTIONS: u32 = 1;
    /// Default query execution timeout in seconds.
    pub const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;
    /// Default page size for paginated tool responses.
    pub const DEFAULT_PAGE_SIZE: u16 = 100;
    /// Maximum accepted value for `page_size`.
    pub const MAX_PAGE_SIZE: u16 = 500;
    /// Default per-cell character cap for query tool rows.
    pub const DEFAULT_MAX_CELL_CHARS: usize = 4096;
    /// Default per-response character cap for query tool rows.
    pub const DEFAULT_MAX_RESPONSE_CHARS: usize = 32_000;

    /// Validates this configuration, accumulating every rule violation.
    ///
    /// Rules enforced:
    /// - `backend == Sqlite` requires `name` to be `Some(non-empty)`.
    /// - `ssl == true` requires every set `ssl_ca` / `ssl_cert` / `ssl_key` path to exist.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per failing rule.
    pub(crate) fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();

        if self.backend == DatabaseBackend::Sqlite && self.name.as_deref().unwrap_or_default().is_empty() {
            errors.push(
                Diagnostic::new(ConfigError::MissingSqliteDbName)
                    .involving(&[env::DB_BACKEND])
                    .missing(&[env::DB_NAME]),
            );
        }

        if self.max_query_timeout > 0 {
            let involving = &[env::DB_QUERY_TIMEOUT, env::DB_MAX_QUERY_TIMEOUT];
            if self.query_timeout == 0 {
                errors.push(Diagnostic::new(ConfigError::MaxQueryTimeoutWithoutTimeout).involving(involving));
            } else if self.max_query_timeout < self.query_timeout {
                errors.push(
                    Diagnostic::new(ConfigError::MaxQueryTimeoutBelowTimeout(
                        self.max_query_timeout,
                        self.query_timeout,
                    ))
                    .involving(involving),
                );
            }
        }

        if self.ssl {
            for (setting, path) in [
                (env::DB_SSL_CA, &self.ssl_ca),
                (env::DB_SSL_CERT, &self.ssl_cert),
                (env::DB_SSL_KEY, &self.ssl_key),
            ] {
                if let Some(path) = path
                    && !std::path::Path::new(path).exists()
                {
                    errors.push(
                        Diagnostic::new(ConfigError::FileNotFound(setting.path, path.clone())).involving(&[setting]),
                    );
                }
            }
        }

        into_result(errors)
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend: Self::DEFAULT_BACKEND,
            host: Self::DEFAULT_HOST.into(),
            port: Self::DEFAULT_BACKEND.default_port(),
            user: Self::DEFAULT_BACKEND.default_user().into(),
            password: None,
            name: None,
            charset: None,
            ssl: Self::DEFAULT_SSL,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            ssl_verify_cert: Self::DEFAULT_SSL_VERIFY_CERT,
            read_only: Self::DEFAULT_READ_ONLY,
            max_pool_size: Self::DEFAULT_MAX_POOL_SIZE,
            connection_timeout: None,
            query_timeout: Self::DEFAULT_QUERY_TIMEOUT_SECS,
            max_query_timeout: Self::DEFAULT_QUERY_TIMEOUT_SECS,
            page_size: Self::DEFAULT_PAGE_SIZE,
            max_cell_chars: Self::DEFAULT_MAX_CELL_CHARS,
            max_response_chars: Self::DEFAULT_MAX_RESPONSE_CHARS,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Config;
    use crate::transport::Transport;

    pub(crate) fn db_config(backend: DatabaseBackend) -> DatabaseConfig {
        DatabaseConfig {
            backend,
            port: backend.default_port(),
            user: backend.default_user().into(),
            ..DatabaseConfig::default()
        }
    }

    fn base_config(backend: DatabaseBackend) -> Config {
        Config {
            database: db_config(backend),
            transport: Transport::Stdio,
            http: None,
            pii: None,
            ssh: None,
            cache: None,
        }
    }

    fn mysql_config() -> Config {
        Config {
            database: DatabaseConfig {
                port: 3306,
                user: "root".into(),
                password: Some("secret".into()),
                ..db_config(DatabaseBackend::Mysql)
            },
            ..base_config(DatabaseBackend::Mysql)
        }
    }

    #[test]
    fn defaults_resolved_at_construction() {
        let mysql = base_config(DatabaseBackend::Mysql);
        assert_eq!(mysql.database.host, "localhost");
        assert_eq!(mysql.database.port, 3306);
        assert_eq!(mysql.database.user, "root");

        let pg = base_config(DatabaseBackend::Postgres);
        assert_eq!(pg.database.port, 5432);
        assert_eq!(pg.database.user, "postgres");

        let sqlite = base_config(DatabaseBackend::Sqlite);
        assert_eq!(sqlite.database.port, 0);
        assert_eq!(sqlite.database.user, "");
    }

    #[test]
    fn explicit_values_override_defaults() {
        let config = Config {
            database: DatabaseConfig {
                host: "dbserver.example.com".into(),
                port: 13306,
                user: "myuser".into(),
                ..db_config(DatabaseBackend::Mysql)
            },
            ..base_config(DatabaseBackend::Mysql)
        };
        assert_eq!(config.database.host, "dbserver.example.com");
        assert_eq!(config.database.port, 13306);
        assert_eq!(config.database.user, "myuser");
    }

    #[test]
    fn mysql_without_user_gets_default() {
        let config = base_config(DatabaseBackend::Mysql);
        assert_eq!(config.database.user, "root");
    }

    #[test]
    fn mariadb_backend_default_user_is_root() {
        let config = base_config(DatabaseBackend::Mariadb);
        assert_eq!(config.database.user, "root");
        assert_eq!(config.database.port, 3306);
    }

    #[test]
    fn page_size_default_is_100() {
        let config = DatabaseConfig::default();
        assert_eq!(config.page_size, 100);
    }

    #[test]
    fn character_caps_default_to_4096_and_32000() {
        let config = DatabaseConfig::default();
        assert_eq!(config.max_cell_chars, 4096);
        assert_eq!(config.max_response_chars, 32_000);
    }

    #[test]
    fn debug_includes_page_size() {
        let config = DatabaseConfig {
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
        let config = |query_timeout, max_query_timeout| DatabaseConfig {
            query_timeout,
            max_query_timeout,
            ..DatabaseConfig::default()
        };
        for (timeout, max) in [(30, 30), (30, 120), (30, 0), (0, 0)] {
            assert!(config(timeout, max).validate().is_ok(), "{timeout}/{max}");
        }

        let errors = config(0, 60).validate().expect_err("cap without a timeout");
        assert!(matches!(errors[0].error, ConfigError::MaxQueryTimeoutWithoutTimeout));
        let errors = config(30, 10).validate().expect_err("cap below the timeout");
        assert!(matches!(
            errors[0].error,
            ConfigError::MaxQueryTimeoutBelowTimeout(10, 30)
        ));
    }

    #[test]
    fn validate_rejects_sqlite_with_none_name() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: None,
            ..DatabaseConfig::default()
        };
        let errors = config.validate().expect_err("sqlite without name must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::MissingSqliteDbName))
        );
    }

    #[test]
    fn validate_rejects_sqlite_with_empty_name() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some(String::new()),
            ..DatabaseConfig::default()
        };
        let errors = config.validate().expect_err("empty sqlite name must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(&e.error, ConfigError::MissingSqliteDbName))
        );
    }

    #[test]
    fn validate_rejects_missing_ssl_ca_path() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_ca: Some("/nonexistent/ca.pem".into()),
            ..DatabaseConfig::default()
        };
        let errors = config.validate().expect_err("missing ssl ca must fail");
        assert!(errors.iter().any(|e| matches!(
            &e.error,
            ConfigError::FileNotFound(name, path)
                if *name == "database.ssl_ca" && path == "/nonexistent/ca.pem"
        )));
    }

    #[test]
    fn validate_collects_all_three_missing_ssl_paths_in_ca_cert_key_order() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_ca: Some("/nope/ca.pem".into()),
            ssl_cert: Some("/nope/cert.pem".into()),
            ssl_key: Some("/nope/key.pem".into()),
            ..DatabaseConfig::default()
        };
        let errors = config.validate().expect_err("three missing files must fail");
        assert_eq!(errors.len(), 3);
        assert!(matches!(&errors[0].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_ca"));
        assert!(matches!(&errors[1].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_cert"));
        assert!(matches!(&errors[2].error, ConfigError::FileNotFound(name, _) if *name == "database.ssl_key"));
    }

    #[test]
    fn validate_skips_ssl_checks_when_ssl_disabled() {
        let config = DatabaseConfig {
            ssl: false,
            ssl_ca: Some("/nonexistent/ca.pem".into()),
            ..DatabaseConfig::default()
        };
        config.validate().expect("ssl off must skip cert checks");
    }

    #[test]
    fn validate_accumulates_sqlite_name_and_ssl_errors() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: None,
            ssl: true,
            ssl_ca: Some("/nope/ca.pem".into()),
            ..DatabaseConfig::default()
        };
        let errors = config.validate().expect_err("must fail");
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
            database: DatabaseConfig {
                query_timeout: 30,
                ..db_config(DatabaseBackend::Mysql)
            },
            ..base_config(DatabaseBackend::Mysql)
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("query_timeout: 30"),
            "expected query_timeout in debug output: {debug}"
        );
    }
}
