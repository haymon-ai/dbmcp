//! Common building blocks reused across transport subcommands.
//!
//! Hosts [`DatabaseArguments`] — a clap argument group bundling every
//! `--db-*` flag so each is defined exactly once and embedded via
//! `#[command(flatten)]` wherever a database connection is needed — and
//! [`create_server`], the backend-selection factory that maps a
//! configured [`DatabaseBackend`] onto the matching concrete adapter.

use clap::Args;
use database_mcp_config::{ConfigError, DatabaseBackend, DatabaseConfig};
use database_mcp_mysql::MysqlHandler;
use database_mcp_postgres::PostgresHandler;
use database_mcp_sqlite::SqliteHandler;
use tracing::info;

pub(crate) use database_mcp_server::Server;

/// Shared database connection flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "Database")]
pub(crate) struct DatabaseArguments {
    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", default_value_t = DatabaseConfig::DEFAULT_BACKEND)]
    pub(crate) backend: DatabaseBackend,

    /// Database host
    #[arg(long = "db-host", env = "DB_HOST", default_value = DatabaseConfig::DEFAULT_HOST)]
    pub(crate) host: String,

    /// Database port (default: backend-dependent)
    #[arg(long = "db-port", env = "DB_PORT")]
    pub(crate) port: Option<u16>,

    /// Database user (default: backend-dependent)
    #[arg(long = "db-user", env = "DB_USER")]
    pub(crate) user: Option<String>,

    /// Database password
    #[arg(long = "db-password", env = "DB_PASSWORD")]
    pub(crate) password: Option<String>,

    /// Database name or `SQLite` file path
    #[arg(long = "db-name", env = "DB_NAME")]
    pub(crate) name: Option<String>,

    /// Character set (MySQL/MariaDB only)
    #[arg(long = "db-charset", env = "DB_CHARSET")]
    pub(crate) charset: Option<String>,

    /// Enable SSL for database connection
    #[arg(
        long = "db-ssl",
        env = "DB_SSL",
        default_value_t = DatabaseConfig::DEFAULT_SSL,
    )]
    pub(crate) ssl: bool,

    /// Path to CA certificate
    #[arg(long = "db-ssl-ca", env = "DB_SSL_CA")]
    pub(crate) ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(long = "db-ssl-cert", env = "DB_SSL_CERT")]
    pub(crate) ssl_cert: Option<String>,

    /// Path to a client key
    #[arg(long = "db-ssl-key", env = "DB_SSL_KEY")]
    pub(crate) ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(
        long = "db-ssl-verify-cert",
        env = "DB_SSL_VERIFY_CERT",
        default_value_t = DatabaseConfig::DEFAULT_SSL_VERIFY_CERT,
    )]
    pub(crate) ssl_verify_cert: bool,

    /// Enable read-only mode
    #[arg(
        long = "db-read-only",
        env = "DB_READ_ONLY",
        default_value_t = DatabaseConfig::DEFAULT_READ_ONLY,
    )]
    pub(crate) read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "db-max-pool-size",
        env = "DB_MAX_POOL_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub(crate) max_pool_size: u32,

    /// Connection timeout in seconds
    #[arg(
        long = "db-connection-timeout",
        env = "DB_CONNECTION_TIMEOUT",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub(crate) connection_timeout: Option<u64>,

    /// Query execution timeout in seconds
    #[arg(
        long = "db-query-timeout",
        env = "DB_QUERY_TIMEOUT",
        default_value_t = DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS,
        value_parser = clap::value_parser!(u64)
    )]
    pub(crate) query_timeout: u64,
}

impl TryFrom<&DatabaseArguments> for DatabaseConfig {
    type Error = Vec<ConfigError>;

    fn try_from(db: &DatabaseArguments) -> Result<Self, Self::Error> {
        let backend = db.backend;
        let config = Self {
            backend,
            host: db.host.clone(),
            port: db.port.unwrap_or_else(|| backend.default_port()),
            user: db.user.clone().unwrap_or_else(|| backend.default_user().into()),
            password: db.password.clone(),
            name: db.name.clone(),
            charset: db.charset.clone(),
            ssl: db.ssl,
            ssl_ca: db.ssl_ca.clone(),
            ssl_cert: db.ssl_cert.clone(),
            ssl_key: db.ssl_key.clone(),
            ssl_verify_cert: db.ssl_verify_cert,
            read_only: db.read_only,
            max_pool_size: db.max_pool_size,
            connection_timeout: db.connection_timeout,
            query_timeout: Some(db.query_timeout),
        };
        config.validate()?;
        Ok(config)
    }
}

/// Logs the read-only banner and builds a [`Server`] for `db_config`.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable. The caller is expected to pass a
/// `db_config` that has already been validated, typically by
/// constructing it via [`DatabaseConfig::try_from`].
#[must_use]
pub(crate) fn create_server(db_config: &DatabaseConfig) -> Server {
    if db_config.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    match db_config.backend {
        DatabaseBackend::Sqlite => SqliteHandler::new(db_config).into(),
        DatabaseBackend::Postgres => PostgresHandler::new(db_config).into(),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => MysqlHandler::new(db_config).into(),
    }
}
