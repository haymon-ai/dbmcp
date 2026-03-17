//! Configuration for the MCP server.
//!
//! All configuration fields live directly on [`Config`] as a flat struct.
//! Database connection is configured via individual variables (`DB_HOST`,
//! `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`, `DB_BACKEND`) instead
//! of a single DSN URL. Values are resolved from three sources with clear
//! precedence: CLI flags > environment variables > `.env` file > defaults.
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
/// Constructed from CLI arguments, environment variables, and `.env` files.
#[derive(Clone)]
pub struct Config {
    /// Database backend type.
    pub db_backend: DatabaseBackend,

    /// Database host.
    pub db_host: Option<String>,

    /// Database port.
    pub db_port: Option<u16>,

    /// Database user.
    pub db_user: Option<String>,

    /// Database password (sensitive — redacted in Debug output).
    pub db_password: Option<String>,

    /// Database name or `SQLite` file path.
    pub db_name: Option<String>,

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

    /// Bind host for HTTP transport (only set when the ` http ` subcommand is used).
    pub http_host: Option<String>,

    /// Bind port for HTTP transport (only set when the ` http ` subcommand is used).
    pub http_port: Option<u16>,

    /// Allowed CORS origins (only set when the ` http ` subcommand is used).
    pub http_allowed_origins: Option<Vec<String>>,

    /// Allowed host names (only set when the ` http ` subcommand is used).
    pub http_allowed_hosts: Option<Vec<String>>,
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
    /// Returns the effective database host, defaulting to `localhost`.
    #[must_use]
    pub fn effective_host(&self) -> &str {
        self.db_host.as_deref().unwrap_or("localhost")
    }

    /// Returns the effective database port, using backend defaults when unset.
    #[must_use]
    pub fn effective_port(&self) -> u16 {
        self.db_port
            .unwrap_or_else(|| self.db_backend.default_port())
    }

    /// Returns the effective database user, using backend defaults when unset.
    #[must_use]
    pub fn effective_user(&self) -> &str {
        self.db_user
            .as_deref()
            .unwrap_or(self.db_backend.default_user())
    }

    /// Validates the configuration and returns all errors found.
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ConfigError>` if any validation rules fail.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        if self.db_backend == DatabaseBackend::Sqlite && self.db_name.is_none() {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(backend: DatabaseBackend) -> Config {
        Config {
            db_backend: backend,
            db_host: None,
            db_port: None,
            db_user: None,
            db_password: None,
            db_name: None,
            db_charset: None,
            db_ssl: false,
            db_ssl_ca: None,
            db_ssl_cert: None,
            db_ssl_key: None,
            db_ssl_verify_cert: true,
            db_read_only: true,
            db_max_pool_size: 10,
            log_level: "info".into(),
            log_file: "logs/mcp_server.log".into(),
            http_host: None,
            http_port: None,
            http_allowed_origins: None,
            http_allowed_hosts: None,
        }
    }

    fn mysql_config() -> Config {
        Config {
            db_host: Some("localhost".into()),
            db_port: Some(3306),
            db_user: Some("root".into()),
            db_password: Some("secret".into()),
            ..base_config(DatabaseBackend::Mysql)
        }
    }

    #[test]
    fn debug_redacts_password() {
        let config = Config {
            db_password: Some("super_secret_password".into()),
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
            db_user: Some("pguser".into()),
            db_port: Some(5432),
            ..base_config(DatabaseBackend::Postgres)
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn valid_sqlite_config_passes() {
        let config = Config {
            db_name: Some("./test.db".into()),
            ..base_config(DatabaseBackend::Sqlite)
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn effective_user_uses_defaults() {
        let mysql = base_config(DatabaseBackend::Mysql);
        assert_eq!(mysql.effective_user(), "root");

        let pg = base_config(DatabaseBackend::Postgres);
        assert_eq!(pg.effective_user(), "postgres");

        let sqlite = base_config(DatabaseBackend::Sqlite);
        assert_eq!(sqlite.effective_user(), "");

        let custom = Config {
            db_user: Some("myuser".into()),
            ..base_config(DatabaseBackend::Mysql)
        };
        assert_eq!(custom.effective_user(), "myuser");
    }

    #[test]
    fn mysql_without_user_passes_validation() {
        let config = base_config(DatabaseBackend::Mysql);
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
    fn effective_port_uses_defaults() {
        let mysql = base_config(DatabaseBackend::Mysql);
        assert_eq!(mysql.effective_port(), 3306);

        let pg = base_config(DatabaseBackend::Postgres);
        assert_eq!(pg.effective_port(), 5432);

        let custom = Config {
            db_port: Some(13306),
            ..base_config(DatabaseBackend::Mysql)
        };
        assert_eq!(custom.effective_port(), 13306);
    }

    #[test]
    fn mariadb_backend_is_valid() {
        let config = base_config(DatabaseBackend::Mariadb);
        assert!(config.validate().is_ok());
    }
}
