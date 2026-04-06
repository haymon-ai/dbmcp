//! Configuration for the MCP server.
//!
//! Configuration is organized into sections:
//! - [`DatabaseConfig`] — database connection and behavior settings
//! - [`HttpConfig`] — HTTP transport binding and security settings
//!
//! The top-level [`Config`] composes these sections. Database connection is
//! configured via individual variables (`DB_HOST`, `DB_PORT`, `DB_USER`,
//! `DB_PASSWORD`, `DB_NAME`, `DB_BACKEND`) instead of a single DSN URL.
//! Values are resolved with clear precedence:
//! CLI flags > environment variables > defaults.
//!
//! All defaults (backend-aware port, user, host) are resolved at construction
//! time in the `From<&Cli>` conversion — consumers access plain values directly.
//!
//! # Security
//!
//! [`DatabaseConfig`] implements [`Debug`] manually to redact the database password.

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

/// Database connection and behavior settings.
///
/// All fields are fully resolved — no `Option` indirection for connection
/// fields. Defaults are applied during construction in `From<&Cli>`.
#[derive(Clone)]
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
    pub password: Option<String>,

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
}

impl std::fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("backend", &self.backend)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("name", &self.name)
            .field("charset", &self.charset)
            .field("ssl", &self.ssl)
            .field("ssl_ca", &self.ssl_ca)
            .field("ssl_cert", &self.ssl_cert)
            .field("ssl_key", &self.ssl_key)
            .field("ssl_verify_cert", &self.ssl_verify_cert)
            .field("read_only", &self.read_only)
            .field("max_pool_size", &self.max_pool_size)
            .field("connection_timeout", &self.connection_timeout)
            .finish()
    }
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
        }
    }
}

/// HTTP transport binding and security settings.
#[derive(Clone, Debug)]
pub struct HttpConfig {
    /// Bind host for HTTP transport.
    pub host: String,

    /// Bind port for HTTP transport.
    pub port: u16,

    /// Allowed CORS origins.
    pub allowed_origins: Vec<String>,

    /// Allowed host names.
    pub allowed_hosts: Vec<String>,
}

impl HttpConfig {
    /// Default HTTP bind host.
    pub const DEFAULT_HOST: &'static str = "127.0.0.1";
    /// Default HTTP bind port.
    pub const DEFAULT_PORT: u16 = 9001;

    /// Return default allowed CORS origins.
    #[must_use]
    pub fn default_allowed_origins() -> Vec<String> {
        vec![
            "http://localhost".into(),
            "http://127.0.0.1".into(),
            "https://localhost".into(),
            "https://127.0.0.1".into(),
        ]
    }

    /// Returns default allowed host names.
    #[must_use]
    pub fn default_allowed_hosts() -> Vec<String> {
        vec!["localhost".into(), "127.0.0.1".into()]
    }
}

/// Runtime configuration for the MCP server.
///
/// Composes [`DatabaseConfig`] with an optional [`HttpConfig`].
/// HTTP config is present only when the HTTP transport is selected
/// (via subcommand or `MCP_TRANSPORT` env var). Logging is configured
/// directly from CLI arguments before `Config` is constructed, so it
/// is not part of this struct.
#[derive(Clone, Debug)]
pub struct Config {
    /// Database connection and behavior settings.
    pub database: DatabaseConfig,

    /// HTTP transport settings (present only when HTTP transport is active).
    pub http: Option<HttpConfig>,
}

impl Config {
    /// Validates the configuration and returns all errors found.
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ConfigError>` if any validation rules fail.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        if self.database.backend == DatabaseBackend::Sqlite
            && self.database.name.as_deref().unwrap_or_default().is_empty()
        {
            errors.push(ConfigError::MissingSqliteDbName);
        }

        if self.database.ssl {
            for (name, path) in [
                ("DB_SSL_CA", &self.database.ssl_ca),
                ("DB_SSL_CERT", &self.database.ssl_cert),
                ("DB_SSL_KEY", &self.database.ssl_key),
            ] {
                if let Some(path) = path
                    && !std::path::Path::new(path).exists()
                {
                    errors.push(ConfigError::SslCertNotFound(name.into(), path.clone()));
                }
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_config(backend: DatabaseBackend) -> DatabaseConfig {
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
            http: None,
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
    fn debug_redacts_password() {
        let config = Config {
            database: DatabaseConfig {
                password: Some("super_secret_password".into()),
                ..mysql_config().database
            },
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
            database: DatabaseConfig {
                user: "pguser".into(),
                port: 5432,
                ..db_config(DatabaseBackend::Postgres)
            },
            ..base_config(DatabaseBackend::Postgres)
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn valid_sqlite_config_passes() {
        let config = Config {
            database: DatabaseConfig {
                name: Some("./test.db".into()),
                ..db_config(DatabaseBackend::Sqlite)
            },
            ..base_config(DatabaseBackend::Sqlite)
        };
        assert!(config.validate().is_ok());
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
        assert!(config.validate().is_ok());
    }

    #[test]
    fn sqlite_requires_db_name() {
        let config = base_config(DatabaseBackend::Sqlite);
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }

    #[test]
    fn multiple_errors_accumulated() {
        let config = Config {
            database: DatabaseConfig {
                ssl: true,
                ssl_ca: Some("/nonexistent/ca.pem".into()),
                ssl_cert: Some("/nonexistent/cert.pem".into()),
                ssl_key: Some("/nonexistent/key.pem".into()),
                ..db_config(DatabaseBackend::Mysql)
            },
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
