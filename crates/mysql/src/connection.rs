//! `MySQL`/`MariaDB` connection: pool cache, pool initialization, and [`Connection`] impl.
//!
//! Owns the lazy default pool and the moka cache of per-database pools.
//! Hides every backend pool concern from [`MysqlHandler`](crate::MysqlHandler),
//! which composes one [`MysqlConnection`] as a field.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_sql::Connection;
use database_mcp_sql::identifier::validate_identifier;
use moka::future::Cache;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode};
use tracing::info;

/// Maximum number of cached per-database connection pools.
pub(crate) const POOL_CACHE_CAPACITY: u64 = 16;

/// Owns every `MySqlPool` the handler uses and the logic that builds them.
#[derive(Clone)]
pub(crate) struct MysqlConnection {
    config: DatabaseConfig,
    default_db: String,
    default_pool: MySqlPool,
    pools: Cache<String, MySqlPool>,
}

impl std::fmt::Debug for MysqlConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlConnection")
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl MysqlConnection {
    /// Builds the connection and its lazy default pool.
    ///
    /// Does **not** establish a database connection. The default pool
    /// connects on demand when the first query is executed. Non-default
    /// database pools are created lazily on first request.
    pub(crate) fn new(config: &DatabaseConfig) -> Self {
        let default_db = config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or_else(String::new, String::from);

        let default_pool = pool_options(config).connect_lazy_with(connect_options(config));

        info!(
            "MySQL lazy connection pool created (max size: {})",
            config.max_pool_size
        );

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: MySqlPool, _cause| {
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
    pub(crate) async fn pool(&self, target: Option<&str>) -> Result<MySqlPool, AppError> {
        let db_key = match target {
            Some(name) if !name.is_empty() => name,
            _ => return Ok(self.default_pool.clone()),
        };

        if !self.default_db.is_empty() && db_key == self.default_db {
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

impl Connection for MysqlConnection {
    type DB = sqlx::MySql;
    const IDENTIFIER_QUOTE: char = '`';

    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, AppError> {
        self.pool(target).await
    }

    fn query_timeout(&self) -> Option<u64> {
        self.config.query_timeout
    }
}

/// Builds [`MySqlPoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> MySqlPoolOptions {
    let mut opts = MySqlPoolOptions::new()
        .max_connections(config.max_pool_size)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> MySqlConnectOptions {
    let mut opts = MySqlConnectOptions::new()
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
    if let Some(ref charset) = config.charset {
        opts = opts.charset(charset);
    }

    if config.ssl {
        opts = if config.ssl_verify_cert {
            opts.ssl_mode(MySqlSslMode::VerifyCa)
        } else {
            opts.ssl_mode(MySqlSslMode::Required)
        };
        if let Some(ref ca) = config.ssl_ca {
            opts = opts.ssl_ca(ca);
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
            backend: DatabaseBackend::Mysql,
            host: "db.example.com".into(),
            port: 3307,
            user: "admin".into(),
            password: Some("s3cret".into()),
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

        assert_eq!(opts.get_host(), "db.example.com");
        assert_eq!(opts.get_port(), 3307);
        assert_eq!(opts.get_username(), "admin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_charset() {
        let config = DatabaseConfig {
            charset: Some("utf8mb4".into()),
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_charset(), "utf8mb4");
    }

    #[test]
    fn try_from_with_ssl_required() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(matches!(opts.get_ssl_mode(), MySqlSslMode::Required));
    }

    #[test]
    fn try_from_with_ssl_verify_ca() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: true,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(matches!(opts.get_ssl_mode(), MySqlSslMode::VerifyCa));
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "db.example.com");
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

    #[tokio::test]
    async fn new_creates_lazy_default_pool() {
        let connection = MysqlConnection::new(&base_config());
        assert_eq!(connection.default_db(), "mydb");
        assert_eq!(connection.default_pool.size(), 0, "default pool should be lazy");
    }

    #[tokio::test]
    async fn defaults_db_to_empty_when_name_missing() {
        let connection = MysqlConnection::new(&DatabaseConfig {
            name: None,
            ..base_config()
        });
        assert_eq!(connection.default_db(), "");
    }

    #[tokio::test]
    async fn none_target_returns_default_pool() {
        let connection = MysqlConnection::new(&base_config());
        connection.pool(None).await.expect("None target should succeed");
    }

    #[tokio::test]
    async fn default_db_target_returns_default_pool() {
        let connection = MysqlConnection::new(&base_config());
        connection
            .pool(Some("mydb"))
            .await
            .expect("default db target should return default pool");
    }

    #[tokio::test]
    async fn arbitrary_target_database_is_permitted() {
        let connection = MysqlConnection::new(&base_config());
        connection
            .pool(Some("any_db"))
            .await
            .expect("any database should be permitted");
    }

    #[tokio::test]
    async fn pool_cache_respects_capacity_const() {
        let connection = MysqlConnection::new(&base_config());

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
