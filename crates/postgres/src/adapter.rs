//! `PostgreSQL` adapter definition and connection configuration.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_sql::identifier::validate_identifier;
use moka::future::Cache;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use tracing::info;

/// Maximum number of database connection pools to cache (including the default).
const POOL_CACHE_CAPACITY: u64 = 6;

/// `PostgreSQL` database adapter.
///
/// All connection pools — including the default — live in a single
/// concurrent cache keyed by database name. No external mutex required.
#[derive(Clone)]
pub struct PostgresAdapter {
    pub(crate) config: DatabaseConfig,
    pub(crate) default_db: String,
    pub(crate) pools: Cache<String, PgPool>,
}

impl std::fmt::Debug for PostgresAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresAdapter")
            .field("read_only", &self.config.read_only)
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl PostgresAdapter {
    /// Creates a new `PostgreSQL` adapter from configuration.
    ///
    /// Stores a clone of the configuration for constructing connection options
    /// for non-default databases at runtime. The initial pool is placed into
    /// the shared cache keyed by the configured database name.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let pool = pool_options(config)
            .connect_with(connect_options(config))
            .await
            .map_err(|e| connection_error("PostgreSQL", config, &e))?;

        info!(
            "PostgreSQL connection pool initialized (max size: {})",
            config.max_pool_size
        );

        // PostgreSQL defaults to a database named after the connecting user.
        let default_db = config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or_else(|| config.user.clone(), String::from);

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: PgPool, _cause| {
                tokio::spawn(async move {
                    pool.close().await;
                });
            })
            .build();

        pools.insert(default_db.clone(), pool).await;

        Ok(Self {
            config: config.clone(),
            default_db,
            pools,
        })
    }

    /// Wraps `name` in double quotes for safe use in `PostgreSQL` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }

    /// Returns a connection pool for the requested database.
    ///
    /// Resolves `None` or empty names to the default pool. On a cache miss
    /// a new pool is created and cached. Evicted pools are closed via the
    /// cache's eviction listener.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidIdentifier`] if the database name fails
    /// validation, or [`AppError::Connection`] if the new pool cannot connect.
    pub(crate) async fn get_pool(&self, database: Option<&str>) -> Result<PgPool, AppError> {
        let db_key = match database {
            Some(name) if !name.is_empty() => name,
            _ => &self.default_db,
        };

        if let Some(pool) = self.pools.get(db_key).await {
            return Ok(pool);
        }

        // Cache miss — validate then create a new pool.
        validate_identifier(db_key)?;

        let config = self.config.clone();
        let db_key_owned = db_key.to_owned();

        let label = format!("PostgreSQL database '{db_key}'");
        let pool = self
            .pools
            .try_get_with(db_key_owned, async {
                let mut cfg = config;
                cfg.name = Some(db_key.to_owned());
                pool_options(&cfg)
                    .connect_with(connect_options(&cfg))
                    .await
                    .map_err(|e| connection_error(&label, &cfg, &e))
            })
            .await
            .map_err(|e| match e.as_ref() {
                AppError::Connection(msg) => AppError::Connection(msg.clone()),
                other => AppError::Connection(other.to_string()),
            })?;

        Ok(pool)
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

/// Formats a connection error with optional timeout hint.
fn connection_error(label: &str, config: &DatabaseConfig, err: &sqlx::Error) -> AppError {
    let timeout_hint = config
        .connection_timeout
        .map_or(String::new(), |t| format!(" (connection timeout: {t}s)"));
    AppError::Connection(format!("Failed to connect to {label}{timeout_hint}: {err}"))
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

        assert!(
            matches!(opts.get_ssl_mode(), PgSslMode::Require),
            "expected Require, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_with_ssl_verify_ca() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: true,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), PgSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
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
}
