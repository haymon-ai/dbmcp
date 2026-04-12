//! `MySQL`/`MariaDB` connection: pool ownership + initialization.
//!
//! Owns the single lazy [`MySqlPool`] used by [`MysqlHandler`](crate::MysqlHandler).
//! Cross-database access is performed at the query layer via per-call
//! `USE` statements on the same acquired pool connection.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_sql::connection::Connection;
use database_mcp_sql::identifier::{quote_identifier, validate_identifier};
use database_mcp_sql::timeout::execute_with_timeout;
use serde_json::Value;
use sqlx::Executor;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode};
use sqlx_to_json::RowExt;
use tracing::info;

/// Owns the lazy `MySQL` pool and the logic that builds it.
#[derive(Clone)]
pub(crate) struct MysqlConnection {
    config: DatabaseConfig,
    pool: MySqlPool,
}

impl std::fmt::Debug for MysqlConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlConnection").finish_non_exhaustive()
    }
}

impl MysqlConnection {
    /// Builds the connection and its lazy pool.
    ///
    /// Does **not** establish a database connection; the pool connects
    /// on demand when the first query is executed.
    pub(crate) fn new(config: &DatabaseConfig) -> Self {
        let pool = pool_options(config).connect_lazy_with(connect_options(config));
        info!(
            "MySQL lazy connection pool created (max size: {})",
            config.max_pool_size
        );
        Self {
            config: config.clone(),
            pool,
        }
    }

    /// Wraps `name` in backticks for safe use in `MySQL` SQL statements.
    #[allow(clippy::unused_self)]
    pub(crate) fn quote_identifier(&self, name: &str) -> String {
        quote_identifier(name, '`')
    }

    /// Wraps a value in single quotes for use as a SQL string literal.
    ///
    /// Escapes internal single quotes by doubling them.
    #[allow(clippy::unused_self)]
    pub(crate) fn quote_string(&self, value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
    }

    /// Returns the single pool. Target is ignored (single-pool backend).
    ///
    /// Crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    #[allow(clippy::unused_async)]
    pub(crate) async fn pool(&self, _target: Option<&str>) -> Result<MySqlPool, AppError> {
        Ok(self.pool.clone())
    }
}

impl Connection for MysqlConnection {
    async fn execute(&self, query: &str, database: Option<&str>) -> Result<u64, AppError> {
        if let Some(db) = database {
            validate_identifier(db)?;
        }
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        let target_owned = database.map(str::to_owned);
        execute_with_timeout(self.config.query_timeout, query, async move {
            let mut conn = pool.acquire().await?;
            if let Some(db) = target_owned.as_deref() {
                let use_sql = format!("USE {}", quote_identifier(db, '`'));
                (&mut *conn).execute(use_sql.as_str()).await?;
            }
            let result = (&mut *conn).execute(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(result.rows_affected())
        })
        .await
    }

    async fn fetch(&self, query: &str, database: Option<&str>) -> Result<Vec<Value>, AppError> {
        if let Some(db) = database {
            validate_identifier(db)?;
        }
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        let target_owned = database.map(str::to_owned);
        execute_with_timeout(self.config.query_timeout, query, async move {
            let mut conn = pool.acquire().await?;
            if let Some(db) = target_owned.as_deref() {
                let use_sql = format!("USE {}", quote_identifier(db, '`'));
                (&mut *conn).execute(use_sql.as_str()).await?;
            }
            let rows = (&mut *conn).fetch_all(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(rows.iter().map(RowExt::to_json).collect())
        })
        .await
    }

    async fn fetch_optional(&self, query: &str, database: Option<&str>) -> Result<Option<Value>, AppError> {
        if let Some(db) = database {
            validate_identifier(db)?;
        }
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        let target_owned = database.map(str::to_owned);
        execute_with_timeout(self.config.query_timeout, query, async move {
            let mut conn = pool.acquire().await?;
            if let Some(db) = target_owned.as_deref() {
                let use_sql = format!("USE {}", quote_identifier(db, '`'));
                (&mut *conn).execute(use_sql.as_str()).await?;
            }
            let row = (&mut *conn).fetch_optional(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(row.as_ref().map(RowExt::to_json))
        })
        .await
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
    async fn new_creates_lazy_pool() {
        let connection = MysqlConnection::new(&base_config());
        assert_eq!(connection.pool.size(), 0, "pool should be lazy");
    }

    #[tokio::test]
    async fn pool_returns_single_pool() {
        let connection = MysqlConnection::new(&base_config());
        connection.pool(None).await.expect("None target should succeed");
        connection
            .pool(Some("anything"))
            .await
            .expect("any target should return the same single pool");
    }
}
