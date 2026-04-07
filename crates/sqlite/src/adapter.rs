//! `SQLite` adapter definition and connection configuration.
//!
//! Creates a lazy connection pool via [`SqlitePoolOptions::connect_lazy_with`].
//! No file I/O happens until the first tool invocation.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tracing::info;

/// `SQLite` file-based database adapter.
///
/// The connection pool is created with [`SqlitePoolOptions::connect_lazy_with`],
/// which defers all file I/O until the first query. Connection errors
/// surface as tool-level errors returned to the MCP client.
#[derive(Clone)]
pub struct SqliteAdapter {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: SqlitePool,
}

impl std::fmt::Debug for SqliteAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteAdapter")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl SqliteAdapter {
    /// Creates a new `SQLite` adapter with a lazy connection pool.
    ///
    /// Does **not** open the database file. The pool connects on-demand
    /// when the first query is executed.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        let pool = pool_options(config).connect_lazy_with(connect_options(config));
        let name = config.name.as_deref().unwrap_or_default();
        info!("SQLite lazy connection pool created: {name}");
        Self {
            config: config.clone(),
            pool,
        }
    }

    /// Wraps `name` in double quotes for safe use in `SQLite` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }
}

/// Builds [`SqlitePoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> SqlitePoolOptions {
    let mut opts = SqlitePoolOptions::new()
        .max_connections(1) // SQLite is a single-writer
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`SqliteConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> SqliteConnectOptions {
    let name = config.name.as_deref().unwrap_or_default();
    SqliteConnectOptions::new().filename(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use database_mcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        }
    }

    #[test]
    fn pool_options_applies_defaults() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), 1, "SQLite must be single-writer");
        assert_eq!(opts.get_min_connections(), DatabaseConfig::DEFAULT_MIN_CONNECTIONS);
        assert_eq!(
            opts.get_idle_timeout(),
            Some(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        );
        assert_eq!(
            opts.get_max_lifetime(),
            Some(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS))
        );
    }

    #[test]
    fn pool_options_applies_connection_timeout() {
        let config = DatabaseConfig {
            connection_timeout: Some(7),
            ..base_config()
        };
        let opts = pool_options(&config);

        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(7));
    }

    #[test]
    fn pool_options_without_connection_timeout_uses_sqlx_default() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn pool_options_ignores_max_pool_size() {
        let config = DatabaseConfig {
            max_pool_size: 20,
            ..base_config()
        };
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), 1, "SQLite must always be single-writer");
    }

    #[test]
    fn try_from_sets_filename() {
        let opts = connect_options(&base_config());

        assert_eq!(opts.get_filename().to_str().expect("valid path"), "test.db");
    }

    #[test]
    fn try_from_empty_name_defaults() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        // Empty string filename — validated elsewhere by Config::validate()
        assert_eq!(opts.get_filename().to_str().expect("valid path"), "");
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let config = base_config();
        let adapter = SqliteAdapter::new(&config);
        // Pool exists but has no active connections (lazy).
        assert_eq!(adapter.pool.size(), 0);
    }
}
