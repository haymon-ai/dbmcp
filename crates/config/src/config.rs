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

    /// HTTP bind host is empty.
    #[error("HTTP_HOST must not be empty")]
    EmptyHttpHost,

    /// `page_size` is outside the accepted range `1..=MAX_PAGE_SIZE`.
    #[error("DB_PAGE_SIZE must be between 1 and {max}, got {value}")]
    PageSizeOutOfRange {
        /// The offending value.
        value: u16,
        /// The inclusive upper bound (`DatabaseConfig::MAX_PAGE_SIZE`).
        max: u16,
    },
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

    /// Query execution timeout in seconds.
    ///
    /// `None` means "use default" (30 s when constructed via CLI).
    /// `Some(0)` disables the timeout entirely.
    pub query_timeout: Option<u64>,

    /// Maximum items returned in a single paginated tool response.
    ///
    /// Applies uniformly to every paginated tool (currently `list_tables`).
    /// Range `1..=500`, enforced by CLI parsing and [`Self::validate`].
    pub page_size: u16,
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
            .field("query_timeout", &self.query_timeout)
            .field("page_size", &self.page_size)
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
    /// Default query execution timeout in seconds.
    pub const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;
    /// Default page size for paginated tool responses.
    pub const DEFAULT_PAGE_SIZE: u16 = 100;
    /// Maximum accepted value for `page_size`.
    pub const MAX_PAGE_SIZE: u16 = 500;

    /// Validates the database configuration and returns all errors found.
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ConfigError>` if any validation rules fail.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        if self.backend == DatabaseBackend::Sqlite && self.name.as_deref().unwrap_or_default().is_empty() {
            errors.push(ConfigError::MissingSqliteDbName);
        }

        if self.ssl {
            for (name, path) in [
                ("DB_SSL_CA", &self.ssl_ca),
                ("DB_SSL_CERT", &self.ssl_cert),
                ("DB_SSL_KEY", &self.ssl_key),
            ] {
                if let Some(path) = path
                    && !std::path::Path::new(path).exists()
                {
                    errors.push(ConfigError::SslCertNotFound(name.into(), path.clone()));
                }
            }
        }

        if !(1..=Self::MAX_PAGE_SIZE).contains(&self.page_size) {
            errors.push(ConfigError::PageSizeOutOfRange {
                value: self.page_size,
                max: Self::MAX_PAGE_SIZE,
            });
        }

        errors.is_empty().then_some(()).ok_or(errors)
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
            query_timeout: None,
            page_size: Self::DEFAULT_PAGE_SIZE,
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
        vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
    }

    /// Validates the HTTP configuration and returns all errors found.
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ConfigError>` if any validation rules fail.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        if self.host.trim().is_empty() {
            errors.push(ConfigError::EmptyHttpHost);
        }

        errors.is_empty().then_some(()).ok_or(errors)
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
        assert!(mysql_config().database.validate().is_ok());
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
        assert!(config.database.validate().is_ok());
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
        assert!(config.database.validate().is_ok());
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
        assert!(config.database.validate().is_ok());
    }

    #[test]
    fn sqlite_requires_db_name() {
        let config = base_config(DatabaseBackend::Sqlite);
        let errors = config
            .database
            .validate()
            .expect_err("sqlite without db name must fail");
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
        let errors = config
            .database
            .validate()
            .expect_err("missing ssl cert files must fail");
        assert!(
            errors.len() >= 3,
            "expected at least 3 errors, got {}: {errors:?}",
            errors.len()
        );
    }

    #[test]
    fn mariadb_backend_is_valid() {
        let config = base_config(DatabaseBackend::Mariadb);
        assert!(config.database.validate().is_ok());
    }

    #[test]
    fn query_timeout_default_is_none() {
        let config = DatabaseConfig::default();
        assert!(config.query_timeout.is_none());
    }

    #[test]
    fn page_size_default_is_100() {
        let config = DatabaseConfig::default();
        assert_eq!(config.page_size, 100);
    }

    #[test]
    fn page_size_zero_rejected() {
        let config = DatabaseConfig {
            page_size: 0,
            ..mysql_config().database
        };
        let errors = config.validate().expect_err("page_size=0 must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::PageSizeOutOfRange { value: 0, max: 500 })),
            "expected PageSizeOutOfRange {{ value: 0, max: 500 }}, got {errors:?}"
        );
    }

    #[test]
    fn page_size_above_max_rejected() {
        let config = DatabaseConfig {
            page_size: 501,
            ..mysql_config().database
        };
        let errors = config.validate().expect_err("page_size above max must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::PageSizeOutOfRange { value: 501, max: 500 })),
            "expected PageSizeOutOfRange {{ value: 501, max: 500 }}, got {errors:?}"
        );
    }

    #[test]
    fn page_size_at_min_accepted() {
        let config = DatabaseConfig {
            page_size: 1,
            ..mysql_config().database
        };
        assert!(config.validate().is_ok(), "page_size=1 must be accepted");
    }

    #[test]
    fn page_size_at_max_accepted() {
        let config = DatabaseConfig {
            page_size: DatabaseConfig::MAX_PAGE_SIZE,
            ..mysql_config().database
        };
        assert!(config.validate().is_ok(), "page_size=MAX_PAGE_SIZE must be accepted");
    }

    #[test]
    fn page_size_errors_accumulate_with_others() {
        let config = Config {
            database: DatabaseConfig {
                page_size: 0,
                ..db_config(DatabaseBackend::Sqlite)
            },
            ..base_config(DatabaseBackend::Sqlite)
        };
        let errors = config
            .database
            .validate()
            .expect_err("multiple errors should be accumulated");
        assert!(
            errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)),
            "expected MissingSqliteDbName in {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::PageSizeOutOfRange { value: 0, .. })),
            "expected PageSizeOutOfRange in {errors:?}"
        );
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

    fn http_config() -> HttpConfig {
        HttpConfig {
            host: HttpConfig::DEFAULT_HOST.into(),
            port: HttpConfig::DEFAULT_PORT,
            allowed_origins: HttpConfig::default_allowed_origins(),
            allowed_hosts: HttpConfig::default_allowed_hosts(),
        }
    }

    #[test]
    fn valid_http_config_passes() {
        assert!(http_config().validate().is_ok());
    }

    #[test]
    fn empty_http_host_rejected() {
        let config = HttpConfig {
            host: String::new(),
            ..http_config()
        };
        let errors = config.validate().expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn whitespace_http_host_rejected() {
        let config = HttpConfig {
            host: "   ".into(),
            ..http_config()
        };
        let errors = config.validate().expect_err("whitespace host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn debug_includes_query_timeout() {
        let config = Config {
            database: DatabaseConfig {
                query_timeout: Some(30),
                ..db_config(DatabaseBackend::Mysql)
            },
            ..base_config(DatabaseBackend::Mysql)
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("query_timeout: Some(30)"),
            "expected query_timeout in debug output: {debug}"
        );
    }
}
