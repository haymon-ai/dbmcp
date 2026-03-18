//! Configuration for the MCP server.
//!
//! All configuration fields live directly on [`Config`] as a flat struct.
//! Database connection is configured via individual variables (`DB_HOST`,
//! `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`, `DB_BACKEND`) instead
//! of a single DSN URL. Values are resolved with clear precedence:
//! CLI flags > environment variables > defaults.
//!
//! All defaults (backend-aware port, user, host) are resolved at construction
//! time in the `From<&Cli>` conversion — consumers access plain values directly.
//!
//! # Security
//!
//! [`Config`] implements [`Debug`] manually to redact the database password.

/// Errors that can occur during configuration validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `DB_NAME` is required for `SQLite`.
    #[error("DB_NAME (file path) is required for SQLite")]
    MissingSqliteDbName,

    /// SSL certificate file not found.
    #[error("{0} file not found: {1}")]
    SslCertNotFound(String, String),
}

/// Supported database backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
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

/// Runtime configuration for the MCP server.
///
/// All fields are fully resolved — no `Option` indirection for database or
/// HTTP fields. Defaults are applied during construction in `From<&Cli>`.
#[derive(Clone)]
pub struct Config {
    /// Database backend type.
    pub db_backend: DatabaseBackend,

    /// Database host (resolved default: `"localhost"`).
    pub db_host: String,

    /// Database port (resolved default: backend-dependent).
    pub db_port: u16,

    /// Database user (resolved default: backend-dependent).
    pub db_user: String,

    /// Database password (resolved default: empty string).
    pub db_password: String,

    /// Database name or `SQLite` file path (resolved default: empty string).
    pub db_name: String,

    /// Character set for MySQL/MariaDB connections.
    pub db_charset: Option<String>,

    /// Enable SSL/TLS for the database connection.
    pub db_ssl: bool,

    /// Path to the CA certificate for SSL.
    pub db_ssl_ca: Option<String>,

    /// Path to the client certificate for SSL.
    pub db_ssl_cert: Option<String>,

    /// Path to the client key for SSL.
    pub db_ssl_key: Option<String>,

    /// Whether to verify the server certificate.
    pub db_ssl_verify_cert: bool,

    /// Whether the server runs in read-only mode.
    pub db_read_only: bool,

    /// Maximum database connection pool size.
    pub db_max_pool_size: u32,

    /// Log level filter (e.g. "info", "debug", "warn").
    pub log_level: String,

    /// Path to the log file.
    pub log_file: String,

    /// Bind host for HTTP transport.
    pub http_host: String,

    /// Bind port for HTTP transport.
    pub http_port: u16,

    /// Allowed CORS origins.
    pub http_allowed_origins: Vec<String>,

    /// Allowed host names.
    pub http_allowed_hosts: Vec<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("db_backend", &self.db_backend)
            .field("db_host", &self.db_host)
            .field("db_port", &self.db_port)
            .field("db_user", &self.db_user)
            .field("db_password", &"[REDACTED]")
            .field("db_name", &self.db_name)
            .field("db_charset", &self.db_charset)
            .field("db_ssl", &self.db_ssl)
            .field("db_ssl_ca", &self.db_ssl_ca)
            .field("db_ssl_cert", &self.db_ssl_cert)
            .field("db_ssl_key", &self.db_ssl_key)
            .field("db_ssl_verify_cert", &self.db_ssl_verify_cert)
            .field("db_read_only", &self.db_read_only)
            .field("db_max_pool_size", &self.db_max_pool_size)
            .field("log_level", &self.log_level)
            .field("log_file", &self.log_file)
            .field("http_host", &self.http_host)
            .field("http_port", &self.http_port)
            .field("http_allowed_origins", &self.http_allowed_origins)
            .field("http_allowed_hosts", &self.http_allowed_hosts)
            .finish()
    }
}

impl Config {
    /// Default database backend.
    pub const DEFAULT_DB_BACKEND: DatabaseBackend = DatabaseBackend::Mysql;
    /// Default database host.
    pub const DEFAULT_DB_HOST: &'static str = "localhost";
    /// Default SSL enabled state.
    pub const DEFAULT_DB_SSL: bool = false;
    /// Default SSL certificate verification.
    pub const DEFAULT_DB_SSL_VERIFY_CERT: bool = true;
    /// Default read-only mode.
    pub const DEFAULT_DB_READ_ONLY: bool = true;
    /// Default connection pool size.
    pub const DEFAULT_DB_MAX_POOL_SIZE: u32 = 10;
    /// Default log level.
    pub const DEFAULT_LOG_LEVEL: &'static str = "info";
    /// Default log file path.
    pub const DEFAULT_LOG_FILE: &'static str = "logs/mcp_server.log";
    /// Default HTTP bind host.
    pub const DEFAULT_HTTP_HOST: &'static str = "127.0.0.1";
    /// Default HTTP bind port.
    pub const DEFAULT_HTTP_PORT: u16 = 9001;
    /// Default allowed CORS origins.
    pub const DEFAULT_HTTP_ALLOWED_ORIGINS: &'static [&'static str] = &[
        "http://localhost",
        "http://127.0.0.1",
        "https://localhost",
        "https://127.0.0.1",
    ];
    /// Default allowed host names.
    pub const DEFAULT_HTTP_ALLOWED_HOSTS: &'static [&'static str] = &["localhost", "127.0.0.1"];

    /// Validates the configuration and returns all errors found.
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ConfigError>` if any validation rules fail.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        if self.db_backend == DatabaseBackend::Sqlite && self.db_name.is_empty() {
            errors.push(ConfigError::MissingSqliteDbName);
        }

        if self.db_ssl {
            for (name, path) in [
                ("DB_SSL_CA", &self.db_ssl_ca),
                ("DB_SSL_CERT", &self.db_ssl_cert),
                ("DB_SSL_KEY", &self.db_ssl_key),
            ] {
                if let Some(path) = path
                    && !std::path::Path::new(path).exists()
                {
                    errors.push(ConfigError::SslCertNotFound(name.into(), path.clone()));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(backend: DatabaseBackend) -> Config {
        Config {
            db_backend: backend,
            db_host: Config::DEFAULT_DB_HOST.into(),
            db_port: backend.default_port(),
            db_user: backend.default_user().into(),
            db_password: String::new(),
            db_name: String::new(),
            db_charset: None,
            db_ssl: false,
            db_ssl_ca: None,
            db_ssl_cert: None,
            db_ssl_key: None,
            db_ssl_verify_cert: true,
            db_read_only: Config::DEFAULT_DB_READ_ONLY,
            db_max_pool_size: Config::DEFAULT_DB_MAX_POOL_SIZE,
            log_level: Config::DEFAULT_LOG_LEVEL.into(),
            log_file: Config::DEFAULT_LOG_FILE.into(),
            http_host: Config::DEFAULT_HTTP_HOST.into(),
            http_port: Config::DEFAULT_HTTP_PORT,
            http_allowed_origins: Config::DEFAULT_HTTP_ALLOWED_ORIGINS
                .iter()
                .map(|&s| s.into())
                .collect(),
            http_allowed_hosts: Config::DEFAULT_HTTP_ALLOWED_HOSTS
                .iter()
                .map(|&s| s.into())
                .collect(),
        }
    }

    fn mysql_config() -> Config {
        Config {
            db_port: 3306,
            db_user: "root".into(),
            db_password: "secret".into(),
            ..base_config(DatabaseBackend::Mysql)
        }
    }

    #[test]
    fn debug_redacts_password() {
        let config = Config {
            db_password: "super_secret_password".into(),
            ..mysql_config()
        };
        let debug_output = format!("{config:?}");
        assert!(
            !debug_output.contains("super_secret_password"),
            "password leaked in debug output: {debug_output}"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "expected [REDACTED] in debug output: {debug_output}"
        );
    }

    #[test]
    fn valid_mysql_config_passes() {
        assert!(mysql_config().validate().is_ok());
    }

    #[test]
    fn valid_postgres_config_passes() {
        let config = Config {
            db_user: "pguser".into(),
            db_port: 5432,
            ..base_config(DatabaseBackend::Postgres)
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn valid_sqlite_config_passes() {
        let config = Config {
            db_name: "./test.db".into(),
            ..base_config(DatabaseBackend::Sqlite)
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn defaults_resolved_at_construction() {
        let mysql = base_config(DatabaseBackend::Mysql);
        assert_eq!(mysql.db_host, "localhost");
        assert_eq!(mysql.db_port, 3306);
        assert_eq!(mysql.db_user, "root");

        let pg = base_config(DatabaseBackend::Postgres);
        assert_eq!(pg.db_port, 5432);
        assert_eq!(pg.db_user, "postgres");

        let sqlite = base_config(DatabaseBackend::Sqlite);
        assert_eq!(sqlite.db_port, 0);
        assert_eq!(sqlite.db_user, "");
    }

    #[test]
    fn explicit_values_override_defaults() {
        let config = Config {
            db_host: "dbserver.example.com".into(),
            db_port: 13306,
            db_user: "myuser".into(),
            ..base_config(DatabaseBackend::Mysql)
        };
        assert_eq!(config.db_host, "dbserver.example.com");
        assert_eq!(config.db_port, 13306);
        assert_eq!(config.db_user, "myuser");
    }

    #[test]
    fn mysql_without_user_gets_default() {
        let config = base_config(DatabaseBackend::Mysql);
        assert_eq!(config.db_user, "root");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn sqlite_requires_db_name() {
        let config = base_config(DatabaseBackend::Sqlite);
        let errors = config.validate().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::MissingSqliteDbName))
        );
    }

    #[test]
    fn multiple_errors_accumulated() {
        let config = Config {
            db_ssl: true,
            db_ssl_ca: Some("/nonexistent/ca.pem".into()),
            db_ssl_cert: Some("/nonexistent/cert.pem".into()),
            db_ssl_key: Some("/nonexistent/key.pem".into()),
            ..base_config(DatabaseBackend::Mysql)
        };
        let errors = config.validate().unwrap_err();
        assert!(
            errors.len() >= 3,
            "expected at least 3 errors, got {}: {errors:?}",
            errors.len()
        );
    }

    #[test]
    fn mariadb_backend_is_valid() {
        let config = base_config(DatabaseBackend::Mariadb);
        assert!(config.validate().is_ok());
    }
}
