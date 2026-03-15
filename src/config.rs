//! Configuration for the MCP server.
//!
//! Configuration is organized into logical sub-groups:
//! - [`McpConfig`] — MCP server behavior (read-only mode, pool size)
//! - [`NetworkConfig`] — CORS allowed origins and hosts
//! - [`LogConfig`] — logging level, file path, rotation
//!
//! Database connection (including SSL/TLS) is configured via a DSN URL
//! string (e.g. `mysql://root@localhost/mydb?ssl-mode=required`) passed
//! through `--database-url`, following the sqlx convention.
//!
//! All values are provided exclusively via CLI flags parsed by [`clap`].
//!
//! # Security
//!
//! [`Config`] implements [`Debug`] manually to redact the database URL
//! (which may contain credentials).

// ---------------------------------------------------------------------------
// McpConfig
// ---------------------------------------------------------------------------

/// MCP server behavior settings.
#[derive(Clone, Debug)]
pub struct McpConfig {
    /// Whether the server runs in read-only mode.
    pub read_only: bool,

    /// Maximum database connection pool size.
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
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    /// Allowed CORS origins.
    pub allowed_origins: Vec<String>,

    /// Allowed host names.
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
#[derive(Clone, Debug)]
pub struct LogConfig {
    /// Log level filter (e.g. "info", "debug", "warn").
    pub level: String,

    /// Path to the log file.
    pub file: String,

    /// Maximum log file size in bytes before rotation.
    pub max_bytes: u64,

    /// Number of rotated log files to keep.
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
/// Constructed from CLI arguments parsed by [`clap`].
#[derive(Clone, Default)]
pub struct Config {
    /// Database connection URL (sqlx DSN format).
    ///
    /// Examples:
    /// - `mysql://root@localhost/mydb`
    /// - `postgres://user:pass@host:5432/db`
    /// - `sqlite:./data.db`
    ///
    /// SSL/TLS options can be appended as query parameters
    /// (e.g. `?ssl-mode=required&ssl-ca=/path/to/ca.pem`).
    pub database_url: String,

    /// MCP server behavior settings.
    pub mcp: McpConfig,

    /// Network and CORS settings.
    pub network: NetworkConfig,

    /// Logging settings.
    pub log: LogConfig,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"[REDACTED]")
            .field("mcp", &self.mcp)
            .field("network", &self.network)
            .field("log", &self.log)
            .finish()
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

        assert!(config.database_url.is_empty());

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
    fn debug_redacts_database_url() {
        let config = Config {
            database_url: "mysql://root:secret@localhost/mydb".into(),
            ..Config::default()
        };
        let debug_output = format!("{config:?}");
        assert!(
            !debug_output.contains("secret"),
            "password leaked in debug output: {debug_output}"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "expected [REDACTED] in debug output: {debug_output}"
        );
    }
}
