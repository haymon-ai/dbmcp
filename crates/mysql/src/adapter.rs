//! MySQL/MariaDB adapter definition and connection configuration.
//!
//! Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`] and creates
//! a lazy connection pool via [`MySqlPoolOptions::connect_lazy_with`].
//! No network I/O happens until the first tool invocation.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use sqlx::MySqlPool;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use tracing::info;

/// MySQL/MariaDB database adapter.
///
/// The connection pool is created with [`MySqlPoolOptions::connect_lazy_with`],
/// which defers all network I/O until the first query. Connection errors
/// surface as tool-level errors returned to the MCP client.
#[derive(Clone)]
pub struct MysqlAdapter {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: MySqlPool,
}

impl std::fmt::Debug for MysqlAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlAdapter")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl MysqlAdapter {
    /// Creates a new `MySQL` adapter with a lazy connection pool.
    ///
    /// Does **not** establish a database connection. The pool connects
    /// on-demand when the first query is executed.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
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
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '`')
    }

    /// Wraps a value in single quotes for use as a SQL string literal.
    ///
    /// Escapes internal single quotes by doubling them.
    pub(crate) fn quote_string(value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
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

        // sqlx defaults acquire_timeout to 30s when not overridden
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

        assert!(
            matches!(opts.get_ssl_mode(), MySqlSslMode::Required),
            "expected Required, got {:?}",
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
            matches!(opts.get_ssl_mode(), MySqlSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        // Should not panic — password is simply omitted
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
        let config = base_config();
        let adapter = MysqlAdapter::new(&config);
        assert!(adapter.config.read_only);
        // Pool exists but has no active connections (lazy).
        assert_eq!(adapter.pool.size(), 0);
    }
}
