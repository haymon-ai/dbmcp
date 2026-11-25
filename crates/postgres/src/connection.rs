//! `PostgreSQL` connection: pool cache, pool initialization, and [`Connection`] impl.
//!
//! Owns a moka cache of lazily-created per-database pools (including the default).
//! Hides every backend pool concern from [`PostgresHandler`](crate::PostgresHandler),
//! which composes one [`PostgresConnection`] as a field.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dbmcp_sql::Connection;
use dbmcp_sql::SqlError;
use dbmcp_sql::support::{self, Line, Variant};
use dbmcp_types::database;
use moka::future::Cache;
use secrecy::ExposeSecret;
use sqlx::postgres::{PgConnectOptions, PgPool, PgSslMode};
use tracing::{debug, info, warn};

use crate::{queries, version};

/// Maximum number of cached per-database connection pools.
pub(crate) const POOL_CACHE_CAPACITY: u64 = 16;

/// Owns every `PgPool` the handler uses and the logic that builds them.
#[derive(Clone)]
pub(crate) struct PostgresConnection {
    config: database::Config,
    pools: Cache<String, PgPool>,
    /// Set once a connection read the server version, shared by every pool.
    checked: Arc<AtomicBool>,
    /// Whether `privacy.rules` exist, for the version tier warning.
    rules: bool,
}

impl std::fmt::Debug for PostgresConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresConnection")
            .field("default_database_name", &self.default_database_name())
            .finish_non_exhaustive()
    }
}

impl PostgresConnection {
    /// Builds the connection with an empty pool cache.
    ///
    /// Does **not** establish a database connection. All pools — including
    /// the default — are created lazily on first request via [`pool`](Self::pool).
    pub(crate) fn new(config: &database::Config, rules: bool) -> Self {
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
            pools,
            checked: Arc::new(AtomicBool::new(false)),
            rules,
        }
    }

    /// Returns the configured default database name, or the username as fallback.
    pub(crate) fn default_database_name(&self) -> &str {
        self.config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.config.user)
    }

    /// Evicts the cached pool for `name`, closing its connections.
    ///
    /// Idempotent — does nothing if the pool was not cached.
    pub(crate) async fn invalidate(&self, name: &str) {
        self.pools.invalidate(name).await;
    }

    /// Closes every cached pool, awaiting each clean shutdown.
    ///
    /// Idempotent — `PgPool::close` is safe to call more than once.
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
    pub(crate) async fn pool(&self, target: Option<&str>) -> Result<PgPool, SqlError> {
        let database = match target {
            Some(name) if !name.is_empty() => name,
            _ => self.default_database_name(),
        };

        if let Some(pool) = self.pools.get(database).await {
            return Ok(pool);
        }

        let pool = self
            .pools
            .get_with(database.to_owned(), async {
                create_lazy_pool(&self.config, database, Arc::clone(&self.checked), self.rules)
            })
            .await;

        Ok(pool)
    }
}

impl Connection for PostgresConnection {
    type DB = sqlx::Postgres;

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
        // 57014 is shared with other cancels; only `statement_timeout` counts.
        error
            .as_database_error()
            .is_some_and(|e| e.code().as_deref() == Some("57014") && e.message().contains("statement timeout"))
    }

    async fn session_id(&self, conn: &mut sqlx::PgConnection) -> Result<Option<u64>, sqlx::Error> {
        let pid = queries::session::id(conn).await?;
        let pid = u64::try_from(pid).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        Ok(Some(pid))
    }

    async fn set_timeout(&self, conn: &mut sqlx::PgConnection, limit: Duration) -> Result<(), sqlx::Error> {
        queries::session::set_timeout(conn, limit).await
    }

    async fn kill(conn: &mut sqlx::PgConnection, id: u64) -> Result<(), sqlx::Error> {
        let pid = i32::try_from(id).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        queries::session::kill(conn, pid).await
    }
}
/// Creates a lazy `PostgreSQL` pool for `db_name`.
///
/// Uses [`PgConnectOptions::new_without_pgpass`] to avoid unintended
/// `PG*` environment variable influence, since our config already
/// resolves values from CLI/env. In read-only mode every connection
/// starts with `default_transaction_read_only = on`, so the server
/// rejects writes through any [`Connection`] method. Also warns once
/// when the server is outside Supported; that check never fails the
/// connection.
fn create_lazy_pool(config: &database::Config, database: &str, checked: Arc<AtomicBool>, rules: bool) -> PgPool {
    let mut conn_ops = PgConnectOptions::new_without_pgpass()
        .host(&config.host)
        .port(config.port)
        .username(&config.user);

    if let Some(ref password) = config.password {
        conn_ops = conn_ops.password(password.expose_secret());
    }
    if !database.is_empty() {
        conn_ops = conn_ops.database(database);
    }

    if config.ssl {
        conn_ops = if config.ssl_verify_cert {
            conn_ops.ssl_mode(PgSslMode::VerifyCa)
        } else {
            conn_ops.ssl_mode(PgSslMode::Require)
        };
        if let Some(ref ca) = config.ssl_ca {
            conn_ops = conn_ops.ssl_root_cert(ca);
        }
        if let Some(ref cert) = config.ssl_cert {
            conn_ops = conn_ops.ssl_client_cert(cert);
        }
        if let Some(ref key) = config.ssl_key {
            conn_ops = conn_ops.ssl_client_key(key);
        }
    }
    if config.read_only {
        conn_ops = conn_ops.options([("default_transaction_read_only", "on")]);
    }

    let mut pool_ops = sqlx::pool::PoolOptions::new()
        .max_connections(config.max_pool_size)
        .min_connections(database::Config::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(database::Config::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(database::Config::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        pool_ops = pool_ops.acquire_timeout(Duration::from_secs(timeout));
    }
    let backend = config.backend;
    pool_ops = pool_ops.after_connect(move |conn: &mut sqlx::PgConnection, _meta| {
        let checked = Arc::clone(&checked);
        Box::pin(async move {
            check_version(conn, &checked, backend, rules).await;
            Ok(())
        })
    });

    pool_ops.connect_lazy_with(conn_ops)
}

/// Warns once per process when the server is outside Supported.
///
/// Sets `checked` only once every read succeeded, so a failed read or probe
/// leaves the check to the next connection.
async fn check_version(conn: &mut sqlx::PgConnection, checked: &AtomicBool, backend: database::Backend, rules: bool) {
    if checked.load(Ordering::Acquire) {
        return;
    }
    let Some(num) = conn.server_version_num() else {
        debug!("could not read the server version; checking again on the next connection");
        return;
    };
    let Ok(text) = queries::session::version(&mut *conn).await else {
        debug!("could not read the server version text; checking again on the next connection");
        return;
    };
    let variant = match version::variant(&text) {
        Some(variant) => Some(variant),
        None => match queries::session::aurora_version(&mut *conn).await {
            Ok(aurora) => aurora.then_some(Variant::AuroraPostgres),
            Err(error) => {
                debug!("could not probe for Aurora ({error}); checking again on the next connection");
                return;
            }
        },
    };
    if checked.swap(true, Ordering::AcqRel) {
        return;
    }
    let tier = support::classify(
        backend,
        Line::from_version_num(num),
        variant,
        version::pre_release(&text),
    );
    if let Some(warning) = tier.warning(rules) {
        warn!("{warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> database::Config {
        database::Config {
            backend: database::Backend::Postgres,
            host: "pg.example.com".into(),
            port: 5433,
            user: "pgadmin".into(),
            password: Some("pgpass".into()),
            name: Some("mydb".into()),
            ..database::Config::default()
        }
    }

    #[tokio::test]
    async fn create_lazy_pool_returns_idle_pool() {
        let pool = create_lazy_pool(&base_config(), "mydb", Arc::default(), false);
        assert_eq!(pool.size(), 0, "pool should be lazy (no connections yet)");
    }

    #[tokio::test]
    async fn create_lazy_pool_without_password() {
        let pool = create_lazy_pool(
            &database::Config {
                password: None,
                ..base_config()
            },
            "mydb",
            Arc::default(),
            false,
        );
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn create_lazy_pool_without_database_name() {
        let pool = create_lazy_pool(
            &database::Config {
                name: None,
                ..base_config()
            },
            "",
            Arc::default(),
            false,
        );
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn default_database_name_derived_from_config() {
        let connection = PostgresConnection::new(&base_config(), false);
        assert_eq!(connection.default_database_name(), "mydb");
    }

    #[tokio::test]
    async fn defaults_db_to_username_when_name_missing() {
        let connection = PostgresConnection::new(
            &database::Config {
                name: None,
                ..base_config()
            },
            false,
        );
        assert_eq!(connection.default_database_name(), "pgadmin");
    }

    #[tokio::test]
    async fn none_target_returns_default_pool() {
        let connection = PostgresConnection::new(&base_config(), false);
        connection.pool(None).await.expect("None target should succeed");
    }

    #[tokio::test]
    async fn arbitrary_target_database_is_permitted() {
        let connection = PostgresConnection::new(&base_config(), false);
        connection
            .pool(Some("any_db"))
            .await
            .expect("any database should be permitted");
    }

    #[tokio::test]
    async fn shutdown_is_noop_and_idempotent_on_lazy_pools() {
        let connection = PostgresConnection::new(&base_config(), false);
        connection.pool(Some("a")).await.expect("pool a");
        connection.pool(Some("b")).await.expect("pool b");
        connection.shutdown().await;
        connection.shutdown().await;
    }

    #[tokio::test]
    async fn pool_cache_respects_capacity_const() {
        let connection = PostgresConnection::new(&base_config(), false);

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
