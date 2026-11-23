//! `MySQL`/`MariaDB` connection: pool cache, pool initialization, and [`Connection`] impl.
//!
//! Owns a moka cache of lazily-created per-database pools (including the default).
//! Hides every backend pool concern from [`MysqlHandler`](crate::MysqlHandler),
//! which composes one [`MysqlConnection`] as a field.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dbmcp_config::{DatabaseBackend, DatabaseConfig};
use dbmcp_sql::Connection;
use dbmcp_sql::SqlError;
use dbmcp_sql::support::{self, Variant};
use moka::future::Cache;
use secrecy::ExposeSecret;
use sqlx::AssertSqlSafe;
use sqlx::mysql::{MySqlConnectOptions, MySqlDatabaseError, MySqlPool, MySqlSslMode};
use tracing::{debug, info, warn};

use crate::flavour::{self, BackendMismatch};

/// Maximum number of cached per-database connection pools.
pub(crate) const POOL_CACHE_CAPACITY: u64 = 16;

/// Owns every `MySqlPool` the handler uses and the logic that builds them.
#[derive(Clone)]
pub(crate) struct MysqlConnection {
    config: DatabaseConfig,
    pools: Cache<String, MySqlPool>,
    /// Set once a connection read the server version, shared by every pool.
    checked: Arc<AtomicBool>,
    /// Whether `privacy.rules` exist, for the version tier warning.
    rules: bool,
}

impl std::fmt::Debug for MysqlConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlConnection")
            .field("default_db", &self.default_database_name())
            .finish_non_exhaustive()
    }
}

impl MysqlConnection {
    /// Builds the connection with an empty pool cache.
    ///
    /// Does **not** establish a database connection. All pools — including
    /// the default — are created lazily on first request via [`pool`](Self::pool).
    pub(crate) fn new(config: &DatabaseConfig, rules: bool) -> Self {
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
            pools,
            checked: Arc::new(AtomicBool::new(false)),
            rules,
        }
    }

    /// Returns the configured default database name, or `""` if none.
    pub(crate) fn default_database_name(&self) -> &str {
        self.config.name.as_deref().filter(|n| !n.is_empty()).unwrap_or("")
    }

    /// Evicts the cached pool for `name`, closing its connections.
    ///
    /// Idempotent — does nothing if the pool was not cached.
    pub(crate) async fn invalidate(&self, name: &str) {
        self.pools.invalidate(name).await;
    }

    /// Closes every cached pool, awaiting each clean shutdown.
    ///
    /// Idempotent — `MySqlPool::close` is safe to call more than once.
    pub(crate) async fn shutdown(&self) {
        for (_, pool) in &self.pools {
            pool.close().await;
        }
    }

    /// Resolves the cached pool for `target`, creating it lazily on miss.
    ///
    /// Kept crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the underlying pool creation fails.
    pub(crate) async fn pool(&self, target: Option<&str>) -> Result<MySqlPool, SqlError> {
        let database = match target {
            Some(name) if !name.is_empty() => name,
            _ => self.default_database_name(),
        };

        if let Some(pool) = self.pools.get(database).await {
            return Ok(pool);
        }

        let pool = self
            .pools
            .get_with(database.to_owned(), async { self.lazy_pool(database) })
            .await;

        Ok(pool)
    }

    /// Creates a lazy `MySQL` pool for `database`.
    ///
    /// Combines pool lifecycle options with backend-specific connect
    /// options into a single lazy pool that establishes connections on
    /// first use. In read-only mode every new connection runs
    /// `SET SESSION TRANSACTION READ ONLY`, so the server rejects writes
    /// through any [`Connection`] method. Until `checked` is set, a new
    /// connection also reads the server version: it warns on a
    /// [`BackendMismatch`], else when the server is outside the Supported
    /// tier. That check never fails the connection.
    fn lazy_pool(&self, database: &str) -> MySqlPool {
        let config = &self.config;
        let checked = Arc::clone(&self.checked);
        let mut conn_ops = MySqlConnectOptions::new()
            .host(&config.host)
            .port(config.port)
            .username(&config.user);

        if let Some(ref password) = config.password {
            conn_ops = conn_ops.password(password.expose_secret());
        }
        if !database.is_empty() {
            conn_ops = conn_ops.database(database);
        }
        if let Some(ref charset) = config.charset {
            conn_ops = conn_ops.charset(charset);
        }

        if config.ssl {
            conn_ops = if config.ssl_verify_cert {
                conn_ops.ssl_mode(MySqlSslMode::VerifyCa)
            } else {
                conn_ops.ssl_mode(MySqlSslMode::Required)
            };
            if let Some(ref ca) = config.ssl_ca {
                conn_ops = conn_ops.ssl_ca(ca);
            }
            if let Some(ref cert) = config.ssl_cert {
                conn_ops = conn_ops.ssl_client_cert(cert);
            }
            if let Some(ref key) = config.ssl_key {
                conn_ops = conn_ops.ssl_client_key(key);
            }
        }

        let mut pool_ops = sqlx::pool::PoolOptions::new()
            .max_connections(config.max_pool_size)
            .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
            .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
            .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

        if let Some(timeout) = config.connection_timeout {
            pool_ops = pool_ops.acquire_timeout(Duration::from_secs(timeout));
        }
        let read_only = config.read_only;
        let backend = config.backend;
        let rules = self.rules;
        pool_ops = pool_ops.after_connect(move |conn, _meta| {
            let checked = Arc::clone(&checked);
            Box::pin(async move {
                if read_only {
                    sqlx::raw_sql("SET SESSION TRANSACTION READ ONLY")
                        .execute(&mut *conn)
                        .await?;
                }
                check_version(conn, &checked, backend, rules).await;
                Ok(())
            })
        });

        pool_ops.connect_lazy_with(conn_ops)
    }
}

/// Warns once per process on a [`BackendMismatch`] or a server outside Supported.
///
/// Sets `checked` only once every read succeeded, so a failed read or probe
/// leaves the check to the next connection.
async fn check_version(conn: &mut sqlx::MySqlConnection, checked: &AtomicBool, backend: DatabaseBackend, rules: bool) {
    if checked.load(Ordering::Acquire) {
        return;
    }
    let Ok(version) = sqlx::query_scalar::<_, String>("SELECT CAST(VERSION() AS CHAR)")
        .fetch_one(&mut *conn)
        .await
    else {
        debug!("could not read the server version; checking again on the next connection");
        return;
    };
    let server = flavour::of(&version);
    let message = if server != backend {
        Some(
            BackendMismatch {
                configured: backend,
                server,
            }
            .to_string(),
        )
    } else if let Some(line) = flavour::line(&version, server) {
        let variant = match flavour::variant(&version) {
            Some(variant) => Some(variant),
            None if backend == DatabaseBackend::Mysql => {
                match sqlx::raw_sql("SHOW VARIABLES LIKE 'aurora_version'")
                    .fetch_optional(&mut *conn)
                    .await
                {
                    Ok(row) => row.map(|_| Variant::AuroraMysql),
                    Err(error) => {
                        debug!("could not probe for Aurora ({error}); checking again on the next connection");
                        return;
                    }
                }
            }
            None => None,
        };
        support::classify(backend, line, variant, false).warning(rules)
    } else {
        debug!("could not parse the server version; skipping the version tier check");
        None
    };
    if !checked.swap(true, Ordering::AcqRel)
        && let Some(message) = message
    {
        warn!("{message}");
    }
}

impl Connection for MysqlConnection {
    type DB = sqlx::MySql;

    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
        self.pool(target).await
    }

    fn query_timeout(&self) -> u64 {
        self.config.query_timeout
    }

    fn read_only(&self) -> bool {
        self.config.read_only
    }

    fn is_timeout(error: &sqlx::Error) -> bool {
        // 3024: MySQL `max_execution_time`; 1969: MariaDB `max_statement_time`.
        error
            .as_database_error()
            .and_then(|e| e.try_downcast_ref::<MySqlDatabaseError>())
            .is_some_and(|e| matches!(e.number(), 3024 | 1969))
    }

    async fn session_id(&self, conn: &mut sqlx::MySqlConnection) -> Result<Option<u64>, sqlx::Error> {
        let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()").fetch_one(conn).await?;
        Ok(Some(id))
    }

    async fn set_timeout(&self, conn: &mut sqlx::MySqlConnection, limit: Duration) -> Result<(), sqlx::Error> {
        let query = match self.config.backend {
            DatabaseBackend::Mariadb => sqlx::query("SET SESSION max_statement_time = ?").bind(limit.as_secs_f64()),
            _ => sqlx::query("SET SESSION max_execution_time = ?")
                .bind(u64::try_from(limit.as_millis()).unwrap_or(u64::MAX)),
        };
        query.execute(conn).await.map(|_| ())
    }

    async fn kill(conn: &mut sqlx::MySqlConnection, id: u64) -> Result<(), sqlx::Error> {
        // `id` is a server-returned integer, not user input. MySQL 9 prepares
        // `KILL QUERY ?`, but sqlx can't decode its parameter type (0xf3).
        match sqlx::raw_sql(AssertSqlSafe(format!("KILL QUERY {id}")))
            .execute(conn)
            .await
        {
            // 1094: unknown thread id, the session already ended.
            Err(error)
                if error
                    .as_database_error()
                    .and_then(|e| e.try_downcast_ref::<MySqlDatabaseError>())
                    .is_some_and(|e| e.number() == 1094) =>
            {
                Ok(())
            }
            result => result.map(|_| ()),
        }
    }
}

/// Quotes `value` as a `MySQL` identifier (backtick-wrapped).
#[must_use]
pub(crate) fn quote_ident(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '`')
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn pool_is_idle_until_first_use() {
        let pool = MysqlConnection::new(&base_config(), false)
            .pool(Some("mydb"))
            .await
            .expect("pool");
        assert_eq!(pool.size(), 0, "pool should be lazy (no connections yet)");
    }

    #[tokio::test]
    async fn pool_without_password() {
        let pool = MysqlConnection::new(
            &DatabaseConfig {
                password: None,
                ..base_config()
            },
            false,
        )
        .pool(Some("mydb"))
        .await
        .expect("pool");
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn pool_without_database_name() {
        let pool = MysqlConnection::new(
            &DatabaseConfig {
                name: None,
                ..base_config()
            },
            false,
        )
        .pool(None)
        .await
        .expect("pool");
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn default_db_derived_from_config() {
        let connection = MysqlConnection::new(&base_config(), false);
        assert_eq!(connection.default_database_name(), "mydb");
    }

    #[tokio::test]
    async fn defaults_db_to_empty_when_name_missing() {
        let connection = MysqlConnection::new(
            &DatabaseConfig {
                name: None,
                ..base_config()
            },
            false,
        );
        assert_eq!(connection.default_database_name(), "");
    }

    #[tokio::test]
    async fn none_target_returns_default_pool() {
        let connection = MysqlConnection::new(&base_config(), false);
        connection.pool(None).await.expect("None target should succeed");
    }

    #[tokio::test]
    async fn default_db_target_returns_default_pool() {
        let connection = MysqlConnection::new(&base_config(), false);
        connection
            .pool(Some("mydb"))
            .await
            .expect("default db target should return default pool");
    }

    #[tokio::test]
    async fn arbitrary_target_database_is_permitted() {
        let connection = MysqlConnection::new(&base_config(), false);
        connection
            .pool(Some("any_db"))
            .await
            .expect("any database should be permitted");
    }

    #[tokio::test]
    async fn shutdown_is_noop_and_idempotent_on_lazy_pools() {
        let connection = MysqlConnection::new(&base_config(), false);
        connection.pool(Some("a")).await.expect("pool a");
        connection.pool(Some("b")).await.expect("pool b");
        connection.shutdown().await;
        connection.shutdown().await;
    }

    #[tokio::test]
    async fn pool_cache_respects_capacity_const() {
        let connection = MysqlConnection::new(&base_config(), false);

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
