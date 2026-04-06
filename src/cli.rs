//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Arguments`] struct (parsed by clap), the [`Command`]
//! subcommand enum, [`LogLevel`] selector, [`From`] implementations
//! that convert parsed arguments into configuration types, and the
//! [`run`] entry point that owns the entire bootstrapping pipeline.

use clap::{Parser, Subcommand};
use database_mcp_config::{Config, DatabaseBackend, DatabaseConfig, HttpConfig};
use std::process::ExitCode;
use tracing::info;

use crate::commands::http::HttpCommand;
use crate::commands::stdio::StdioCommand;
use crate::consts::{BIN, VERSION};
use crate::error::Error;
use crate::server::create_handler;

/// Log severity levels for the MCP server.
///
/// Maps directly to [`tracing::Level`] variants. Used as a
/// [`clap::ValueEnum`] for type-safe CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogLevel {
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

/// Top-level CLI arguments parsed by clap.
#[derive(Debug, Parser)]
#[command(name = "database-mcp", about = "Database MCP Server", version)]
struct Arguments {
    /// Subcommand selector.
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
        long = "db-read-only",
        env = "DB_READ_ONLY",
        default_value_t = DatabaseConfig::DEFAULT_READ_ONLY,
        global = true
    )]
    db_read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "db-max-pool-size",
        env = "DB_MAX_POOL_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE,
        global = true,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    db_max_pool_size: u32,

    /// Connection timeout in seconds
    #[arg(
        long = "db-connection-timeout",
        env = "DB_CONNECTION_TIMEOUT",
        global = true,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    db_connection_timeout: Option<u64>,

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
enum Command {
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
            read_only: arguments.db_read_only,
            max_pool_size: arguments.db_max_pool_size,
            connection_timeout: arguments.db_connection_timeout,
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
pub async fn run() -> Result<ExitCode, Error> {
    let arguments = Arguments::parse();
    if matches!(arguments.command, Some(Command::Version)) {
        println!("{BIN} {VERSION}");
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

    fn parse(args: &[&str]) -> Arguments {
        Arguments::try_parse_from(args).unwrap()
    }

    #[test]
    fn no_subcommand_defaults_to_stdio() {
        let args = parse(&[BIN]);
        assert!(args.command.is_none());
        assert!(Config::from(&args).http.is_none());
    }

    #[test]
    fn http_subcommand_populates_http_config() {
        let args = parse(&[BIN, "http", "--port", "8080"]);
        let config = Config::from(&args);
        assert!(config.http.is_some());
        assert_eq!(config.http.as_ref().unwrap().port, 8080);
    }

    #[test]
    fn http_subcommand_uses_default_port() {
        let args = parse(&[BIN, "http"]);
        let config = Config::from(&args);
        assert_eq!(config.http.as_ref().unwrap().port, HttpConfig::DEFAULT_PORT);
    }

    #[test]
    fn db_backend_after_http_subcommand() {
        let args = parse(&[BIN, "http", "--db-backend", "mysql"]);
        assert_eq!(args.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(args.command, Some(Command::Http(_))));
    }

    #[test]
    fn db_backend_before_http_subcommand() {
        let args = parse(&[BIN, "--db-backend", "mysql", "http"]);
        assert_eq!(args.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(args.command, Some(Command::Http(_))));
    }

    #[test]
    fn db_backend_with_no_subcommand() {
        let args = parse(&[BIN, "--db-backend", "postgres"]);
        assert_eq!(args.db_backend, DatabaseBackend::Postgres);
        assert!(args.command.is_none());
    }

    #[test]
    fn multiple_global_args_after_subcommand() {
        let args = parse(&[
            BIN,
            "http",
            "--db-backend",
            "mysql",
            "--db-user",
            "root",
            "--db-name",
            "mydb",
        ]);
        assert_eq!(args.db_backend, DatabaseBackend::Mysql);
        assert_eq!(args.db_user, Some("root".into()));
        assert_eq!(args.db_name, Some("mydb".into()));
    }

    #[test]
    fn db_backend_defaults_to_mysql() {
        let args = parse(&[BIN, "http"]);
        assert_eq!(args.db_backend, DatabaseBackend::Mysql);
    }

    #[test]
    fn cli_flag_overrides_default_backend() {
        let args = parse(&[BIN, "http", "--db-backend", "postgres"]);
        assert_eq!(args.db_backend, DatabaseBackend::Postgres);
    }

    #[test]
    fn db_read_only_flag() {
        let args = parse(&[BIN, "--db-read-only"]);
        assert!(args.db_read_only);
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        let args = parse(&[BIN]);
        assert!(args.db_read_only);
    }

    #[test]
    fn db_max_pool_size_flag() {
        let args = parse(&[BIN, "--db-max-pool-size", "20"]);
        assert_eq!(args.db_max_pool_size, 20);
    }

    #[test]
    fn db_connection_timeout_flag() {
        let args = parse(&[BIN, "--db-connection-timeout", "5"]);
        assert_eq!(args.db_connection_timeout, Some(5));
    }

    #[test]
    fn db_connection_timeout_defaults_to_none() {
        let args = parse(&[BIN]);
        assert_eq!(args.db_connection_timeout, None);
    }

    #[test]
    fn db_connection_timeout_zero_rejected() {
        assert!(Arguments::try_parse_from([BIN, "--db-connection-timeout", "0"]).is_err());
    }

    #[test]
    fn db_connection_timeout_wired_to_config() {
        let args = parse(&[BIN, "--db-connection-timeout", "10"]);
        let config = Config::from(&args);
        assert_eq!(config.database.connection_timeout, Some(10));
    }

    #[test]
    fn valid_log_levels() {
        for level in ["error", "warn", "info", "debug", "trace"] {
            let args = parse(&[BIN, "--log-level", level]);
            assert_eq!(args.log_level.to_string(), level);
        }
    }

    #[test]
    fn invalid_log_level_rejected() {
        assert!(Arguments::try_parse_from([BIN, "--log-level", "nonsense"]).is_err());
    }

    #[test]
    fn log_level_defaults_to_info() {
        let args = parse(&[BIN]);
        assert_eq!(args.log_level, LogLevel::Info);
    }

    #[test]
    fn log_level_case_insensitive() {
        for level in ["DEBUG", "Info", "TRACE", "Warn", "ERROR"] {
            assert!(
                Arguments::try_parse_from([BIN, "--log-level", level]).is_ok(),
                "expected '{level}' to be accepted case-insensitively"
            );
        }
    }

    #[test]
    fn version_subcommand() {
        let args = parse(&[BIN, "version"]);
        assert!(matches!(args.command, Some(Command::Version)));
    }

    #[test]
    fn version_flag() {
        let err = Arguments::try_parse_from([BIN, "--version"]).expect_err("--version should cause clap to return Err");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn invalid_cli_args_rejected() {
        assert!(Arguments::try_parse_from([BIN, "--nonexistent-flag"]).is_err());
    }
}
