//! Configuration loading from environment variables and `.env` files.
//!
//! Configuration is organized into logical sub-groups:
//! - [`DatabaseConfig`] — connection settings (host, port, credentials)
//! - [`SslConfig`] — TLS/SSL certificate and verification options
//! - [`McpConfig`] — MCP server behavior (read-only mode, pool size)
//! - [`NetworkConfig`] — CORS allowed origins and hosts
//! - [`LogConfig`] — logging level, file path, rotation
//!
//! Environment variables are deserialized via the [`envy`] crate using serde.
//! Each sub-struct uses `#[serde(default)]` at the struct level so that its
//! `Default` impl is the single source of truth for all default values.
//!
//! # Validation
//!
//! After deserialization, [`Config::validate`] checks business rules
//! (non-empty credentials, valid port range, pool size > 0) and returns
//! [`ConfigError`] on failure.
//!
//! # Security
//!
//! [`DatabaseConfig`] and [`SslConfig`] implement [`Debug`] manually to
//! redact sensitive fields (`password`, SSL `key` path).

use serde::Deserialize;
use tracing::warn;

// ---------------------------------------------------------------------------
// Custom deserializers
// ---------------------------------------------------------------------------

/// Deserializes a boolean accepting true/false, 1/0, yes/no (case-insensitive).
fn deserialize_bool_lenient<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "invalid boolean value '{other}': expected true/false, 1/0, or yes/no"
        ))),
    }
}

/// Deserializes a comma-separated string into `Vec<String>`, trimming
/// whitespace and filtering empty entries.
fn deserialize_comma_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(s.split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect())
}

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// Errors that can occur during configuration loading or validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required configuration field is missing or empty.
    #[error("missing required config field: {field}")]
    MissingField {
        /// Name of the missing field.
        field: &'static str,
    },

    /// A configuration value could not be parsed.
    #[error("invalid value '{value}' for {field}: expected {expected}")]
    ParseError {
        /// Name of the field.
        field: &'static str,
        /// The raw value that failed to parse.
        value: String,
        /// Description of the expected format.
        expected: &'static str,
    },

    /// A business-rule validation failed.
    #[error("config validation error: {message}")]
    Validation {
        /// Description of the validation failure.
        message: String,
    },

    /// The `envy` crate failed to deserialize environment variables.
    #[error("failed to load config from environment: {0}")]
    EnvyError(#[from] envy::Error),
}

// ---------------------------------------------------------------------------
// DatabaseConfig
// ---------------------------------------------------------------------------

/// Database connection settings.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Database host address.
    #[serde(rename = "db_host")]
    pub host: String,

    /// Database port number.
    #[serde(rename = "db_port")]
    pub port: u16,

    /// Database user name (required).
    #[serde(rename = "db_user")]
    pub user: String,

    /// Database password (required).
    #[serde(rename = "db_password")]
    pub password: String,

    /// Default database name.
    #[serde(rename = "db_name")]
    pub name: Option<String>,

    /// Connection character set.
    #[serde(rename = "db_charset")]
    pub charset: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3306,
            user: String::new(),
            password: String::new(),
            name: None,
            charset: None,
        }
    }
}

impl std::fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("name", &self.name)
            .field("charset", &self.charset)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SslConfig
// ---------------------------------------------------------------------------

/// TLS/SSL connection settings.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SslConfig {
    /// Whether SSL is enabled.
    #[serde(rename = "db_ssl", deserialize_with = "deserialize_bool_lenient")]
    pub enabled: bool,

    /// Path to the CA certificate file.
    #[serde(rename = "db_ssl_ca")]
    pub ca: Option<String>,

    /// Path to the client certificate file.
    #[serde(rename = "db_ssl_cert")]
    pub cert: Option<String>,

    /// Path to the client private key file.
    #[serde(rename = "db_ssl_key")]
    pub key: Option<String>,

    /// Whether to verify the server certificate.
    #[serde(
        rename = "db_ssl_verify_cert",
        deserialize_with = "deserialize_bool_lenient"
    )]
    pub verify_cert: bool,

    /// Whether to verify the server hostname.
    #[serde(
        rename = "db_ssl_verify_identity",
        deserialize_with = "deserialize_bool_lenient"
    )]
    pub verify_identity: bool,
}

impl Default for SslConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ca: None,
            cert: None,
            key: None,
            verify_cert: true,
            verify_identity: false,
        }
    }
}

impl std::fmt::Debug for SslConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SslConfig")
            .field("enabled", &self.enabled)
            .field("ca", &self.ca)
            .field("cert", &self.cert)
            .field("key", &"[REDACTED]")
            .field("verify_cert", &self.verify_cert)
            .field("verify_identity", &self.verify_identity)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// McpConfig
// ---------------------------------------------------------------------------

/// MCP server behavior settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Whether the server runs in read-only mode.
    #[serde(
        rename = "mcp_read_only",
        deserialize_with = "deserialize_bool_lenient"
    )]
    pub read_only: bool,

    /// Maximum database connection pool size.
    #[serde(rename = "mcp_max_pool_size")]
    pub max_pool_size: u32,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            read_only: true,
            max_pool_size: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// NetworkConfig
// ---------------------------------------------------------------------------

/// Network and CORS settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Allowed CORS origins (comma-separated in env var).
    #[serde(
        rename = "allowed_origins",
        deserialize_with = "deserialize_comma_list"
    )]
    pub allowed_origins: Vec<String>,

    /// Allowed host names (comma-separated in env var).
    #[serde(rename = "allowed_hosts", deserialize_with = "deserialize_comma_list")]
    pub allowed_hosts: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec![
                "http://localhost".into(),
                "http://127.0.0.1".into(),
                "https://localhost".into(),
                "https://127.0.0.1".into(),
            ],
            allowed_hosts: vec!["localhost".into(), "127.0.0.1".into()],
        }
    }
}

// ---------------------------------------------------------------------------
// LogConfig
// ---------------------------------------------------------------------------

/// Logging settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// Log level filter (e.g. "info", "debug", "warn").
    #[serde(rename = "log_level")]
    pub level: String,

    /// Path to the log file.
    #[serde(rename = "log_file")]
    pub file: String,

    /// Maximum log file size in bytes before rotation.
    #[serde(rename = "log_max_bytes")]
    pub max_bytes: u64,

    /// Number of rotated log files to keep.
    #[serde(rename = "log_backup_count")]
    pub backup_count: u32,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            file: "logs/mcp_server.log".into(),
            max_bytes: 10_485_760,
            backup_count: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// Config (top-level)
// ---------------------------------------------------------------------------

/// Runtime configuration for the MCP server.
///
/// All sub-groups are flattened so that environment variable names remain
/// unchanged (e.g. `DB_HOST`, `MCP_READ_ONLY`, `LOG_LEVEL`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    /// Database connection settings.
    #[serde(flatten)]
    pub database: DatabaseConfig,

    /// TLS/SSL settings.
    #[serde(flatten)]
    pub ssl: SslConfig,

    /// MCP server behavior settings.
    #[serde(flatten)]
    pub mcp: McpConfig,

    /// Network and CORS settings.
    #[serde(flatten)]
    pub network: NetworkConfig,

    /// Logging settings.
    #[serde(flatten)]
    pub log: LogConfig,
}

impl Config {
    /// Loads configuration from environment variables and validates.
    ///
    /// Deserializes all `DB_*`, `MCP_*`, `ALLOWED_*`, and `LOG_*` environment
    /// variables into a [`Config`] via [`envy`], then runs [`Config::validate`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if deserialization or validation fails.
    pub fn from_env() -> Result<Self, ConfigError> {
        let config: Self = envy::from_env()?;
        config.validate()?;
        Ok(config)
    }

    /// Loads configuration from environment variables without validation.
    ///
    /// Use this when CLI overrides will be applied before validation.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EnvyError`] if deserialization fails.
    pub fn from_env_without_validation() -> Result<Self, ConfigError> {
        Ok(envy::from_env()?)
    }

    /// Validates business rules after deserialization.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingField`] if `DB_USER` or `DB_PASSWORD`
    /// is empty, [`ConfigError::Validation`] if `MCP_MAX_POOL_SIZE` is zero
    /// or `DB_PORT` is out of range.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.database.user.is_empty() {
            return Err(ConfigError::MissingField { field: "DB_USER" });
        }
        if self.database.password.is_empty() {
            return Err(ConfigError::MissingField {
                field: "DB_PASSWORD",
            });
        }
        if self.mcp.max_pool_size == 0 {
            return Err(ConfigError::Validation {
                message: "MCP_MAX_POOL_SIZE must be greater than 0".into(),
            });
        }
        if self.database.port == 0 {
            return Err(ConfigError::Validation {
                message: "DB_PORT must be between 1 and 65535".into(),
            });
        }

        // Warn (but don't error) on incomplete SSL configuration
        if self.ssl.enabled
            && self.ssl.ca.is_none()
            && self.ssl.cert.is_none()
            && self.ssl.key.is_none()
        {
            warn!(
                "DB_SSL is enabled but no certificate paths (DB_SSL_CA, DB_SSL_CERT, DB_SSL_KEY) are set"
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_produces_expected_values() {
        let config = Config::default();

        assert_eq!(config.database.host, "127.0.0.1");
        assert_eq!(config.database.port, 3306);
        assert!(config.database.user.is_empty());
        assert!(config.database.password.is_empty());
        assert!(config.database.name.is_none());
        assert!(config.database.charset.is_none());

        assert!(!config.ssl.enabled);
        assert!(config.ssl.ca.is_none());
        assert!(config.ssl.cert.is_none());
        assert!(config.ssl.key.is_none());
        assert!(config.ssl.verify_cert);
        assert!(!config.ssl.verify_identity);

        assert!(config.mcp.read_only);
        assert_eq!(config.mcp.max_pool_size, 10);

        assert_eq!(config.network.allowed_origins.len(), 4);
        assert_eq!(config.network.allowed_hosts.len(), 2);

        assert_eq!(config.log.level, "info");
        assert_eq!(config.log.file, "logs/mcp_server.log");
        assert_eq!(config.log.max_bytes, 10_485_760);
        assert_eq!(config.log.backup_count, 5);
    }

    #[test]
    fn validate_missing_user() {
        let config = Config {
            database: DatabaseConfig {
                password: "secret".into(),
                ..DatabaseConfig::default()
            },
            ..Config::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingField { field: "DB_USER" }),
            "expected MissingField for DB_USER, got: {err}"
        );
    }

    #[test]
    fn validate_missing_password() {
        let config = Config {
            database: DatabaseConfig {
                user: "root".into(),
                ..DatabaseConfig::default()
            },
            ..Config::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::MissingField {
                    field: "DB_PASSWORD"
                }
            ),
            "expected MissingField for DB_PASSWORD, got: {err}"
        );
    }

    #[test]
    fn validate_zero_pool_size() {
        let config = Config {
            database: DatabaseConfig {
                user: "root".into(),
                password: "pass".into(),
                ..DatabaseConfig::default()
            },
            mcp: McpConfig {
                max_pool_size: 0,
                ..McpConfig::default()
            },
            ..Config::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::Validation { .. }),
            "expected Validation error, got: {err}"
        );
    }

    #[test]
    fn debug_redacts_password() {
        let config = Config {
            database: DatabaseConfig {
                user: "admin".into(),
                password: "super_secret_password_123".into(),
                ..DatabaseConfig::default()
            },
            ..Config::default()
        };
        let debug_output = format!("{config:?}");
        assert!(
            !debug_output.contains("super_secret_password_123"),
            "password leaked in debug output: {debug_output}"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "expected [REDACTED] in debug output: {debug_output}"
        );
    }

    #[test]
    fn debug_redacts_ssl_key() {
        let config = Config {
            ssl: SslConfig {
                key: Some("/path/to/secret.key".into()),
                ..SslConfig::default()
            },
            ..Config::default()
        };
        let debug_output = format!("{config:?}");
        assert!(
            !debug_output.contains("/path/to/secret.key"),
            "SSL key path leaked in debug output: {debug_output}"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "expected [REDACTED] in debug output: {debug_output}"
        );
    }

    #[test]
    fn deserialize_bool_lenient_values() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct TestBool {
            #[serde(deserialize_with = "super::deserialize_bool_lenient")]
            val: bool,
        }

        let truthy = ["true", "TRUE", "True", "1", "yes", "YES", "Yes"];
        let falsy = ["false", "FALSE", "False", "0", "no", "NO", "No"];

        for input in truthy {
            let iter = vec![("VAL".to_string(), input.to_string())];
            let result: TestBool = envy::from_iter(iter).unwrap();
            assert!(result.val, "expected true for input '{input}'");
        }

        for input in falsy {
            let iter = vec![("VAL".to_string(), input.to_string())];
            let result: TestBool = envy::from_iter(iter).unwrap();
            assert!(!result.val, "expected false for input '{input}'");
        }
    }

    #[test]
    fn deserialize_comma_list_trims_and_filters() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct TestList {
            #[serde(deserialize_with = "super::deserialize_comma_list")]
            items: Vec<String>,
        }

        let iter = vec![("ITEMS".to_string(), "a, b , ,c".to_string())];
        let result: TestList = envy::from_iter(iter).unwrap();
        assert_eq!(result.items, vec!["a", "b", "c"]);
    }

    #[test]
    fn valid_config_passes_validation() {
        let config = Config {
            database: DatabaseConfig {
                user: "root".into(),
                password: "pass".into(),
                ..DatabaseConfig::default()
            },
            ..Config::default()
        };
        assert!(config.validate().is_ok());
    }
}
