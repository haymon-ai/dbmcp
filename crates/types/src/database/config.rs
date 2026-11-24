//! Database connection and behavior settings.

use secrecy::SecretString;

use super::Backend;

/// Database connection and behavior settings.
///
/// All fields are fully resolved — no `Option` indirection for connection
/// fields. Defaults are applied by the loader.
#[derive(Clone, Debug)]
pub struct Config {
    /// Database backend type.
    pub backend: Backend,

    /// Database host (resolved default: `"localhost"`).
    pub host: String,

    /// Database port (resolved default: backend-dependent).
    pub port: u16,

    /// Database user (resolved default: backend-dependent).
    pub user: String,

    /// Database password.
    pub password: Option<SecretString>,

    /// Database name or `SQLite` file path.
    pub name: Option<String>,

    /// Character set for MySQL/MariaDB connections.
    pub charset: Option<String>,

    /// Enable SSL/TLS for the database connection.
    pub ssl: bool,

    /// Path to the CA certificate for SSL.
    pub ssl_ca: Option<String>,

    /// Path to the client certificate for SSL.
    pub ssl_cert: Option<String>,

    /// Path to the client key for SSL.
    pub ssl_key: Option<String>,

    /// Whether to verify the server certificate.
    pub ssl_verify_cert: bool,

    /// Whether the server runs in read-only mode.
    pub read_only: bool,

    /// Maximum database connection pool size.
    pub max_pool_size: u32,

    /// Connection timeout in seconds (`None` = driver default).
    pub connection_timeout: Option<u64>,

    /// Query execution timeout in seconds.
    ///
    /// Defaults to [`Self::DEFAULT_QUERY_TIMEOUT_SECS`]. `0` disables dbmcp's
    /// server and client timeouts; server defaults apply.
    pub query_timeout: u64,

    /// Largest per-call `readQuery` or `explainQuery` timeout in seconds (`0` = no cap).
    ///
    /// Defaults to [`Self::query_timeout`], so callers can only shorten it.
    pub max_query_timeout: u64,

    /// Maximum items returned in a single paginated tool response.
    ///
    /// Applies uniformly to every paginated tool (currently `list_tables`).
    /// Range `1..=500`, enforced by the loader.
    pub page_size: u16,

    /// Maximum characters per string value in query tool rows.
    ///
    /// Longer values are cut and marked. `0` disables the cap.
    pub max_cell_chars: usize,

    /// Maximum characters of one query tool text result.
    ///
    /// Trailing rows past the budget are dropped: a paged `SELECT` resumes
    /// them via its cursor, other calls count them. `0` disables the cap.
    pub max_response_chars: usize,
}

impl Config {
    /// Default database backend.
    pub const DEFAULT_BACKEND: Backend = Backend::Mysql;
    /// Default database host.
    pub const DEFAULT_HOST: &'static str = "localhost";
    /// Default SSL enabled state.
    pub const DEFAULT_SSL: bool = false;
    /// Default SSL certificate verification.
    pub const DEFAULT_SSL_VERIFY_CERT: bool = true;
    /// Default read-only mode.
    pub const DEFAULT_READ_ONLY: bool = true;
    /// Default connection pool size.
    pub const DEFAULT_MAX_POOL_SIZE: u32 = 5;
    /// Default idle timeout in seconds (10 minutes).
    pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;
    /// Default max lifetime in seconds (30 minutes).
    pub const DEFAULT_MAX_LIFETIME_SECS: u64 = 1800;
    /// Default minimum connections in pool.
    pub const DEFAULT_MIN_CONNECTIONS: u32 = 1;
    /// Default query execution timeout in seconds.
    pub const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;
    /// Default page size for paginated tool responses.
    pub const DEFAULT_PAGE_SIZE: u16 = 100;
    /// Maximum accepted value for `page_size`.
    pub const MAX_PAGE_SIZE: u16 = 500;
    /// Default per-cell character cap for query tool rows.
    pub const DEFAULT_MAX_CELL_CHARS: usize = 4096;
    /// Default per-response character cap for query tool rows.
    pub const DEFAULT_MAX_RESPONSE_CHARS: usize = 32_000;
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: Self::DEFAULT_BACKEND,
            host: Self::DEFAULT_HOST.into(),
            port: Self::DEFAULT_BACKEND.default_port(),
            user: Self::DEFAULT_BACKEND.default_user().into(),
            password: None,
            name: None,
            charset: None,
            ssl: Self::DEFAULT_SSL,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            ssl_verify_cert: Self::DEFAULT_SSL_VERIFY_CERT,
            read_only: Self::DEFAULT_READ_ONLY,
            max_pool_size: Self::DEFAULT_MAX_POOL_SIZE,
            connection_timeout: None,
            query_timeout: Self::DEFAULT_QUERY_TIMEOUT_SECS,
            max_query_timeout: Self::DEFAULT_QUERY_TIMEOUT_SECS,
            page_size: Self::DEFAULT_PAGE_SIZE,
            max_cell_chars: Self::DEFAULT_MAX_CELL_CHARS,
            max_response_chars: Self::DEFAULT_MAX_RESPONSE_CHARS,
        }
    }
}
