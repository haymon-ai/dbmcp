//! Configuration for the MCP server.
//!
//! All configuration fields live directly on [`Config`] as a flat struct —
//! no sub-structs. Database connection (including SSL/TLS) is configured
//! via a DSN URL string (e.g. `mysql://root@localhost/mydb?ssl-mode=required`)
//! passed through `--database-url`, following the sqlx convention.
//!
//! All values are provided exclusively via CLI flags parsed by [`clap`].
//!
//! # Security
//!
//! [`Config`] implements [`Debug`] manually to redact the database URL
//! (which may contain credentials).

/// Runtime configuration for the MCP server.
///
/// Constructed from CLI arguments parsed by [`clap`].
#[derive(Clone)]
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

    /// Whether the server runs in read-only mode.
    pub read_only: bool,

    /// Maximum database connection pool size.
    pub max_pool_size: u32,

    /// Allowed CORS origins.
    pub allowed_origins: Vec<String>,

    /// Allowed host names.
    pub allowed_hosts: Vec<String>,

    /// Log level filter (e.g. "info", "debug", "warn").
    pub log_level: String,

    /// Path to the log file.
    pub log_file: String,

    /// Maximum log file size in bytes before rotation.
    pub log_max_bytes: u64,

    /// Number of rotated log files to keep.
    pub log_backup_count: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            read_only: true,
            max_pool_size: 10,
            allowed_origins: vec![
                "http://localhost".into(),
                "http://127.0.0.1".into(),
                "https://localhost".into(),
                "https://127.0.0.1".into(),
            ],
            allowed_hosts: vec!["localhost".into(), "127.0.0.1".into()],
            log_level: "info".into(),
            log_file: "logs/mcp_server.log".into(),
            log_max_bytes: 10_485_760,
            log_backup_count: 5,
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"[REDACTED]")
            .field("read_only", &self.read_only)
            .field("max_pool_size", &self.max_pool_size)
            .field("allowed_origins", &self.allowed_origins)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("log_level", &self.log_level)
            .field("log_file", &self.log_file)
            .field("log_max_bytes", &self.log_max_bytes)
            .field("log_backup_count", &self.log_backup_count)
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

        assert!(config.read_only);
        assert_eq!(config.max_pool_size, 10);

        assert_eq!(config.allowed_origins.len(), 4);
        assert_eq!(config.allowed_hosts.len(), 2);

        assert_eq!(config.log_level, "info");
        assert_eq!(config.log_file, "logs/mcp_server.log");
        assert_eq!(config.log_max_bytes, 10_485_760);
        assert_eq!(config.log_backup_count, 5);
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
