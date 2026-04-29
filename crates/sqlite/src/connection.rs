//! `SQLite` connection: pool ownership, initialization, and [`Connection`] impl.
//!
//! Owns the single lazy [`SqlitePool`] used by [`SqliteHandler`](crate::SqliteHandler).
//! `SQLite` is a single-file, single-writer backend; the pool is fixed
//! at one connection.

use std::time::Duration;

use dbmcp_config::DatabaseConfig;
use dbmcp_sql::Connection;
use dbmcp_sql::SqlError;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tracing::info;

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
        info!("SQLite lazy connection pool created");

        Self {
            config: config.clone(),
            pool: create_lazy_pool(config),
        }
    }

    /// Returns the single pool. Target is ignored (`SQLite` is single-file).
    ///
    /// Crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    #[allow(clippy::unused_async)]
    pub(crate) async fn pool(&self, _target: Option<&str>) -> Result<SqlitePool, SqlError> {
        Ok(self.pool.clone())
    }
}

impl Connection for SqliteConnection {
    type DB = sqlx::Sqlite;

    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
        self.pool(target).await
    }

    fn query_timeout(&self) -> Option<u64> {
        self.config.query_timeout
    }
}
/// Creates a lazy `SQLite` pool from a [`DatabaseConfig`].
///
/// Forces `max_connections` to 1 — `SQLite` is a single-writer backend.
fn create_lazy_pool(config: &DatabaseConfig) -> SqlitePool {
    let conn_ops = SqliteConnectOptions::new().filename(config.name.as_deref().unwrap_or_default());
    let mut pool_opts = sqlx::pool::PoolOptions::new()
        .max_connections(1)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        pool_opts = pool_opts.acquire_timeout(Duration::from_secs(timeout));
    }

    pool_opts.connect_lazy_with(conn_ops)
}

/// Quotes `value` as a `SQLite` identifier (ANSI double-quote style).
#[must_use]
pub(crate) fn quote_ident(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbmcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        }
    }

    #[tokio::test]
    async fn create_lazy_pool_returns_idle_pool() {
        let pool = create_lazy_pool(&base_config());
        assert_eq!(pool.size(), 0, "pool should be lazy (no connections yet)");
    }

    #[tokio::test]
    async fn create_lazy_pool_without_name() {
        let pool = create_lazy_pool(&DatabaseConfig {
            name: None,
            ..base_config()
        });
        assert_eq!(pool.size(), 0);
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
