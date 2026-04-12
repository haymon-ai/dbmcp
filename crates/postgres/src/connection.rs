//! `PostgreSQL` connection: pool cache, pool initialization, and [`Connection`] impl.
//!
//! Owns the lazy default pool and the moka cache of per-database pools.
//! Hides every backend pool concern from [`PostgresHandler`](crate::PostgresHandler),
//! which composes one [`PostgresConnection`] as a field.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_sql::Connection;
use database_mcp_sql::identifier::validate_identifier;
use moka::future::Cache;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use tracing::info;

/// Maximum number of cached per-database connection pools.
pub(crate) const POOL_CACHE_CAPACITY: u64 = 16;

/// Owns every `PgPool` the handler uses and the logic that builds them.
#[derive(Clone)]
pub(crate) struct PostgresConnection {
    config: DatabaseConfig,
    default_db: String,
    default_pool: PgPool,
    pools: Cache<String, PgPool>,
}

impl std::fmt::Debug for PostgresConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresConnection")
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl PostgresConnection {
    /// Builds the connection and its lazy default pool.
    ///
    /// Does **not** establish a database connection. The default pool
    /// connects on demand when the first query is executed. Non-default
    /// database pools are created lazily on first request.
    pub(crate) fn new(config: &DatabaseConfig) -> Self {
        // PostgreSQL defaults to a database named after the connecting user.
        let default_db = config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or_else(|| config.user.clone(), String::from);

        let default_pool = pool_options(config).connect_lazy_with(connect_options(config));

        info!(
            "PostgreSQL lazy connection pool created (max size: {})",
            config.max_pool_size
        );

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: PgPool, _cause| {
                tokio::spawn(async move {
                    pool.close().await;
                });
            })
            .build();

        Self {
            config: config.clone(),
            default_db,
            default_pool,
            pools,
        }
    }

    /// Returns the name of the database resolved at startup.
    pub(crate) fn default_db(&self) -> &str {
        &self.default_db
    }

    /// Evicts the cached pool for `name`, closing its connections.
    ///
    /// Idempotent — does nothing if the pool was not cached.
    pub(crate) async fn invalidate(&self, name: &str) {
        self.pools.invalidate(name).await;
    }

    /// Resolves the cached pool for `target`, creating it lazily on miss.
    ///
    /// Kept crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    ///
    /// # Errors
    ///
    /// - [`AppError::InvalidIdentifier`] — `target` failed identifier validation.
    pub(crate) async fn pool(&self, target: Option<&str>) -> Result<PgPool, AppError> {
        let db_key = match target {
            Some(name) if !name.is_empty() => name,
            _ => return Ok(self.default_pool.clone()),
        };

        if db_key == self.default_db {
            return Ok(self.default_pool.clone());
        }

        if let Some(pool) = self.pools.get(db_key).await {
            return Ok(pool);
        }

        validate_identifier(db_key)?;

        let config = self.config.clone();
        let db_key_owned = db_key.to_owned();

        let pool = self
            .pools
            .get_with(db_key_owned, async {
                let mut cfg = config;
                cfg.name = Some(db_key.to_owned());
                pool_options(&cfg).connect_lazy_with(connect_options(&cfg))
            })
            .await;

        Ok(pool)
    }
}

impl Connection for PostgresConnection {
    type DB = sqlx::Postgres;
    const IDENTIFIER_QUOTE: char = '"';

    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, AppError> {
        self.pool(target).await
    }

    fn query_timeout(&self) -> Option<u64> {
        self.config.query_timeout
    }
}

/// Builds [`PgPoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> PgPoolOptions {
    let mut opts = PgPoolOptions::new()
        .max_connections(config.max_pool_size)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`PgConnectOptions`] from a [`DatabaseConfig`].
///
/// Uses [`PgConnectOptions::new_without_pgpass`] to avoid unintended
/// `PG*` environment variable influence, since our config already
/// resolves values from CLI/env.
fn connect_options(config: &DatabaseConfig) -> PgConnectOptions {
    let mut opts = PgConnectOptions::new_without_pgpass()
        .host(&config.host)
        .port(config.port)
        .username(&config.user);

    if let Some(ref password) = config.password {
        opts = opts.password(password);
    }
    if let Some(ref name) = config.name
        && !name.is_empty()
    {
        opts = opts.database(name);
    }

    if config.ssl {
        opts = if config.ssl_verify_cert {
            opts.ssl_mode(PgSslMode::VerifyCa)
        } else {
            opts.ssl_mode(PgSslMode::Require)
        };
        if let Some(ref ca) = config.ssl_ca {
            opts = opts.ssl_root_cert(ca);
        }
        if let Some(ref cert) = config.ssl_cert {
            opts = opts.ssl_client_cert(cert);
        }
        if let Some(ref key) = config.ssl_key {
            opts = opts.ssl_client_key(key);
        }
    }

    opts
}

#[cfg(test)]
mod tests {
    use super::*;
    use database_mcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Postgres,
            host: "pg.example.com".into(),
            port: 5433,
            user: "pgadmin".into(),
            password: Some("pgpass".into()),
            name: Some("mydb".into()),
            ..DatabaseConfig::default()
        }
    }

    #[test]
    fn pool_options_applies_defaults() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), config.max_pool_size);
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
    fn try_from_basic_config() {
        let config = base_config();
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
        assert_eq!(opts.get_port(), 5433);
        assert_eq!(opts.get_username(), "pgadmin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_ssl_require() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(matches!(opts.get_ssl_mode(), PgSslMode::Require));
    }

    #[test]
    fn try_from_with_ssl_verify_ca() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: true,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(matches!(opts.get_ssl_mode(), PgSslMode::VerifyCa));
    }

    #[test]
    fn try_from_without_database_name() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_database(), None);
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
    }

    #[tokio::test]
    async fn new_creates_lazy_default_pool() {
        let connection = PostgresConnection::new(&base_config());
        assert_eq!(connection.default_db(), "mydb");
        assert_eq!(connection.default_pool.size(), 0, "default pool should be lazy");
    }

    #[tokio::test]
    async fn defaults_db_to_username_when_name_missing() {
        let connection = PostgresConnection::new(&DatabaseConfig {
            name: None,
            ..base_config()
        });
        assert_eq!(connection.default_db(), "pgadmin");
    }

    #[tokio::test]
    async fn none_target_returns_default_pool() {
        let connection = PostgresConnection::new(&base_config());
        connection.pool(None).await.expect("None target should succeed");
    }

    #[tokio::test]
    async fn arbitrary_target_database_is_permitted() {
        let connection = PostgresConnection::new(&base_config());
        connection
            .pool(Some("any_db"))
            .await
            .expect("any database should be permitted");
    }

    #[tokio::test]
    async fn pool_cache_respects_capacity_const() {
        let connection = PostgresConnection::new(&base_config());

        // Insert one more pool than the cap; moka should evict the
        // oldest so the cached count stays at or below POOL_CACHE_CAPACITY.
        for i in 0..=POOL_CACHE_CAPACITY {
            let name = format!("db_{i}");
            connection.pool(Some(&name)).await.expect("pool should succeed");
        }
        connection.pools.run_pending_tasks().await;

        assert!(
            connection.pools.entry_count() <= POOL_CACHE_CAPACITY,
            "cached pools exceeded cap: {} > {POOL_CACHE_CAPACITY}",
            connection.pools.entry_count()
        );
    }
}
