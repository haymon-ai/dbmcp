//! CLI argument parsing and application bootstrapping.
//!
//! This module owns the entire bootstrapping pipeline: CLI argument parsing
//! (via clap with subcommands), tracing initialization, configuration
//! construction, validation, database backend creation, and MCP transport
//! dispatch.
//!
//! The binary has three subcommands:
//! - `stdio` (default) — runs the MCP server over stdin/stdout
//! - `http` — runs the MCP server over HTTP with Streamable HTTP transport
//! - `version` — prints the version and exits

use config::{Config, DatabaseBackend, DatabaseConfig, HttpConfig};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorData, ListToolsResult, PaginatedRequestParams, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use std::process::ExitCode;
use tracing::info;

use super::http::HttpCommand;
use super::stdio::StdioCommand;

use clap::{Parser, Subcommand};

/// Application-level errors for server startup and transport.
///
/// Only instantiated once at program exit, so variant size is irrelevant.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub enum RunError {
    /// Database backend initialization failed.
    #[error(transparent)]
    Backend(#[from] backend::AppError),

    /// MCP transport failed to initialize.
    #[error("transport error: {0}")]
    Transport(#[from] rmcp::service::ServerInitializeError),

    /// Network I/O error (e.g., TCP bind failure).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Missing or invalid configuration at runtime.
    #[error("{0}")]
    Config(String),
}

/// Log severity levels for the MCP server.
///
/// Maps directly to [`tracing::Level`] variants. Used as a
/// [`clap::ValueEnum`] for type-safe CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LogLevel {
    /// Only errors.
    Error,
    /// Warnings and above.
    Warn,
    /// Informational and above (default).
    Info,
    /// Debug and above.
    Debug,
    /// All trace output.
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
            Self::Trace => write!(f, "trace"),
        }
    }
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

#[derive(Parser)]
#[command(name = "database-mcp", about = "Database MCP Server", version)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,

    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", default_value_t = DatabaseConfig::DEFAULT_BACKEND, global = true)]
    db_backend: DatabaseBackend,

    /// Database host
    #[arg(long = "db-host", env = "DB_HOST", default_value = DatabaseConfig::DEFAULT_HOST, global = true)]
    db_host: String,

    /// Database port (default: backend-dependent)
    #[arg(long = "db-port", env = "DB_PORT", global = true)]
    db_port: Option<u16>,

    /// Database user (default: backend-dependent)
    #[arg(long = "db-user", env = "DB_USER", global = true)]
    db_user: Option<String>,

    /// Database password
    #[arg(long = "db-password", env = "DB_PASSWORD", global = true)]
    db_password: Option<String>,

    /// Database name or `SQLite` file path
    #[arg(long = "db-name", env = "DB_NAME", global = true)]
    db_name: Option<String>,

    /// Character set (MySQL/MariaDB only)
    #[arg(long = "db-charset", env = "DB_CHARSET", global = true)]
    db_charset: Option<String>,

    /// Enable SSL for database connection
    #[arg(
        long = "db-ssl",
        env = "DB_SSL",
        default_value_t = DatabaseConfig::DEFAULT_SSL,
        global = true
    )]
    db_ssl: bool,

    /// Path to CA certificate
    #[arg(long = "db-ssl-ca", env = "DB_SSL_CA", global = true)]
    db_ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(long = "db-ssl-cert", env = "DB_SSL_CERT", global = true)]
    db_ssl_cert: Option<String>,

    /// Path to a client key
    #[arg(long = "db-ssl-key", env = "DB_SSL_KEY", global = true)]
    db_ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(
        long = "db-ssl-verify-cert",
        env = "DB_SSL_VERIFY_CERT",
        default_value_t = DatabaseConfig::DEFAULT_SSL_VERIFY_CERT,
        global = true
    )]
    db_ssl_verify_cert: bool,

    /// Enable read-only mode
    #[arg(
        long = "read-only",
        env = "MCP_READ_ONLY",
        default_value_t = DatabaseConfig::DEFAULT_READ_ONLY,
        global = true
    )]
    read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "max-pool-size",
        env = "MCP_MAX_POOL_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE,
        global = true,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    max_pool_size: u32,

    /// Log level
    #[arg(
        long = "log-level",
        env = "LOG_LEVEL",
        default_value_t = LogLevel::Info,
        ignore_case = true,
        global = true
    )]
    log_level: LogLevel,
}

/// Top-level subcommand selector.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print version information and exit.
    Version,
    /// Run in stdio mode (default).
    Stdio(StdioCommand),
    /// Run in HTTP/SSE mode.
    Http(HttpCommand),
}

impl From<&Arguments> for DatabaseConfig {
    fn from(arguments: &Arguments) -> Self {
        let backend = arguments.db_backend;
        Self {
            backend,
            host: arguments.db_host.clone(),
            port: arguments.db_port.unwrap_or_else(|| backend.default_port()),
            user: arguments
                .db_user
                .clone()
                .unwrap_or_else(|| backend.default_user().into()),
            password: arguments.db_password.clone(),
            name: arguments.db_name.clone(),
            charset: arguments.db_charset.clone(),
            ssl: arguments.db_ssl,
            ssl_ca: arguments.db_ssl_ca.clone(),
            ssl_cert: arguments.db_ssl_cert.clone(),
            ssl_key: arguments.db_ssl_key.clone(),
            ssl_verify_cert: arguments.db_ssl_verify_cert,
            read_only: arguments.read_only,
            max_pool_size: arguments.max_pool_size,
        }
    }
}

impl From<&Command> for Option<HttpConfig> {
    fn from(cmd: &Command) -> Self {
        match cmd {
            Command::Http(http) => Some(HttpConfig {
                host: http.host.clone(),
                port: http.port,
                allowed_origins: http.allowed_origins.clone(),
                allowed_hosts: http.allowed_hosts.clone(),
            }),
            _ => None,
        }
    }
}

impl From<&Arguments> for Config {
    fn from(arguments: &Arguments) -> Self {
        Self {
            database: DatabaseConfig::from(arguments),
            http: arguments.command.as_ref().and_then(Into::into),
        }
    }
}

/// Unified handler enum dispatching to the active backend.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum Handler {
    Sqlite(sqlite::SqliteHandler),
    Postgres(postgres::PostgresHandler),
    Mysql(mysql::MysqlHandler),
}

/// Delegates a [`ServerHandler`] method call to the inner handler.
macro_rules! dispatch {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Handler::Sqlite(h) => h.$method($($arg),*),
            Handler::Postgres(h) => h.$method($($arg),*),
            Handler::Mysql(h) => h.$method($($arg),*),
        }
    };
    (await $self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Handler::Sqlite(h) => h.$method($($arg),*).await,
            Handler::Postgres(h) => h.$method($($arg),*).await,
            Handler::Mysql(h) => h.$method($($arg),*).await,
        }
    };
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerInfo {
        dispatch!(self, get_info)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        dispatch!(await self, list_tools, request, context)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch!(await self, call_tool, request, context)
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        dispatch!(self, get_tool, name)
    }
}

/// Creates a [`Handler`] based on the configured database backend.
async fn create_handler(config: &Config) -> Result<Handler, backend::AppError> {
    let handler = match config.database.backend {
        DatabaseBackend::Sqlite => Handler::Sqlite(sqlite::SqliteHandler::new(&config.database).await?),
        DatabaseBackend::Postgres => Handler::Postgres(postgres::PostgresHandler::new(&config.database).await?),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => {
            Handler::Mysql(mysql::MysqlHandler::new(&config.database).await?)
        }
    };
    Ok(handler)
}

/// Parses CLI arguments, initializes the application, and runs the MCP server.
///
/// # Errors
///
/// Returns an error if:
/// - Configuration validation fails (missing/invalid values).
/// - Database connection fails (invalid URL, unreachable host, auth failure).
/// - TCP bind fails for HTTP transport (port in use, permission denied).
/// - MCP stdio transport fails to start.
#[tokio::main]
#[allow(clippy::result_large_err)]
pub async fn run() -> Result<ExitCode, RunError> {
    let arguments = Arguments::parse();
    if matches!(arguments.command, Some(Command::Version)) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from(arguments.log_level))
        .with_ansi(false)
        .init();

    let config = Config::from(&arguments);
    if let Err(errors) = config.validate() {
        eprintln!("Error: configuration validation failed:");
        for error in &errors {
            eprintln!("  - {error}");
        }
        return Ok(ExitCode::FAILURE);
    }

    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    let handler = create_handler(&config).await?;
    match &arguments.command {
        Some(Command::Http(cmd)) => cmd.execute(&config, handler).await?,
        _ => StdioCommand.execute(handler).await?,
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_db_backend_after_http_subcommand() {
        let arguments = Arguments::try_parse_from(["database-mcp", "http", "--db-backend", "mysql"]).unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(arguments.command, Some(Command::Http(_))));
    }

    #[test]
    fn parse_db_backend_before_http_subcommand() {
        let arguments = Arguments::try_parse_from(["database-mcp", "--db-backend", "mysql", "http"]).unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(arguments.command, Some(Command::Http(_))));
    }

    #[test]
    fn parse_db_backend_with_no_subcommand() {
        let arguments = Arguments::try_parse_from(["database-mcp", "--db-backend", "postgres"]).unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Postgres);
        assert!(arguments.command.is_none());
    }

    #[test]
    fn parse_multiple_global_args_after_subcommand() {
        let arguments = Arguments::try_parse_from([
            "database-mcp",
            "http",
            "--db-backend",
            "mysql",
            "--db-user",
            "root",
            "--db-name",
            "mydb",
        ])
        .unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Mysql);
        assert_eq!(arguments.db_user, Some("root".into()));
        assert_eq!(arguments.db_name, Some("mydb".into()));
    }

    #[test]
    fn parse_db_backend_defaults_to_mysql() {
        let arguments = Arguments::try_parse_from(["database-mcp", "http"]).unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Mysql);
    }

    #[test]
    fn cli_flag_overrides_default_backend() {
        let arguments = Arguments::try_parse_from(["database-mcp", "http", "--db-backend", "postgres"]).unwrap();
        assert_eq!(arguments.db_backend, DatabaseBackend::Postgres);
    }

    #[test]
    fn parse_valid_log_levels() {
        for level in ["error", "warn", "info", "debug", "trace"] {
            let arguments = Arguments::try_parse_from(["database-mcp", "--log-level", level]).unwrap();
            assert_eq!(arguments.log_level.to_string(), level);
        }
    }

    #[test]
    fn parse_invalid_log_level_is_rejected() {
        assert!(Arguments::try_parse_from(["database-mcp", "--log-level", "nonsense"]).is_err());
    }

    #[test]
    fn log_level_defaults_to_info() {
        let arguments = Arguments::try_parse_from(["database-mcp"]).unwrap();
        assert_eq!(arguments.log_level, LogLevel::Info);
    }

    #[test]
    fn parse_log_level_case_insensitive() {
        for level in ["DEBUG", "Info", "TRACE", "Warn", "ERROR"] {
            assert!(
                Arguments::try_parse_from(["database-mcp", "--log-level", level]).is_ok(),
                "expected '{level}' to be accepted case-insensitively"
            );
        }
    }

    #[test]
    fn version_flag_is_accepted() {
        let result = Arguments::try_parse_from(["database-mcp", "--version"]);
        // clap exits early for --version, so try_parse_from returns an Err
        // with DisplayVersion kind — not a "real" error.
        let err = result.err().expect("--version should cause clap to return Err");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn short_version_flag_is_accepted() {
        let err = Arguments::try_parse_from(["database-mcp", "-V"])
            .err()
            .expect("-V should cause clap to return Err");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn version_subcommand_is_parsed() {
        let arguments = Arguments::try_parse_from(["database-mcp", "version"]).unwrap();
        assert!(matches!(arguments.command, Some(Command::Version)));
    }
}
