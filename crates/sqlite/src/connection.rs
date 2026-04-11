//! `SQLite` connection: pool ownership + initialization.
//!
//! Owns the single lazy [`SqlitePool`] used by [`SqliteHandler`](crate::SqliteHandler).
//! `SQLite` is a single-file, single-writer backend; the pool is fixed
//! at one connection.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_sql::connection::{Connection, Executable, Query};
use database_mcp_sql::timeout::execute_with_timeout;
use sqlx::Sqlite;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteQueryResult};

/// Owns the lazy `SQLite` pool and the logic that builds it.
#[derive(Clone)]
pub(crate) struct SqliteConnection {
    config: DatabaseConfig,
    pool: SqlitePool,
}

impl std::fmt::Debug for SqliteConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteConnection").finish_non_exhaustive()
    }
}

impl SqliteConnection {
    /// Builds the connection and its lazy pool.
    pub(crate) fn new(config: &DatabaseConfig) -> Self {
        Self {
            config: config.clone(),
            pool: pool_options(config).connect_lazy_with(connect_options(config)),
        }
    }

    /// Wraps `name` in double quotes for safe use in `SQLite` SQL statements.
    #[allow(clippy::unused_self)]
    pub(crate) fn quote_identifier(&self, name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }

    /// Returns the single pool. Target is ignored (`SQLite` is single-file).
    ///
    /// Crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    #[allow(clippy::unused_async)]
    pub(crate) async fn pool(&self, _target: Option<&str>) -> Result<SqlitePool, AppError> {
        Ok(self.pool.clone())
    }
}

impl Connection for SqliteConnection {
    type Database = Sqlite;

    async fn execute<Q>(&self, query: Q, target: Option<&str>) -> Result<SqliteQueryResult, AppError>
    where
        Q: Executable<Sqlite>,
    {
        let pool = self.pool(target).await?;
        let sql = query.sql().to_owned();
        execute_with_timeout(self.config.query_timeout, &sql, async move {
            let mut conn = pool.acquire().await?;
            query.run_execute(&mut conn).await
        })
        .await
    }

    async fn fetch<Q>(&self, query: Q, target: Option<&str>) -> Result<Vec<Q::Output>, AppError>
    where
        Q: Query<Sqlite>,
    {
        let pool = self.pool(target).await?;
        let sql = query.sql().to_owned();
        execute_with_timeout(self.config.query_timeout, &sql, async move {
            let mut conn = pool.acquire().await?;
            query.run_fetch_all(&mut conn).await
        })
        .await
    }

    async fn fetch_optional<Q>(&self, query: Q, target: Option<&str>) -> Result<Option<Q::Output>, AppError>
    where
        Q: Query<Sqlite>,
    {
        let pool = self.pool(target).await?;
        let sql = query.sql().to_owned();
        execute_with_timeout(self.config.query_timeout, &sql, async move {
            let mut conn = pool.acquire().await?;
            query.run_fetch_optional(&mut conn).await
        })
        .await
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
    SqliteConnectOptions::new().filename(config.name.as_deref().unwrap_or_default())
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
        assert_eq!(opts.get_filename().to_str().expect("valid path"), "");
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let connection = SqliteConnection::new(&base_config());
        assert_eq!(connection.pool.size(), 0, "pool should be lazy");
    }

    #[tokio::test]
    async fn pool_returns_single_pool() {
        let connection = SqliteConnection::new(&base_config());
        connection.pool(None).await.expect("None target should succeed");
        connection
            .pool(Some("anything"))
            .await
            .expect("any target should return the same single pool");
    }
}
