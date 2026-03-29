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

use database_mcp::config::{Config, DatabaseBackend, DatabaseConfig, HttpConfig};
use database_mcp::db;
use database_mcp::db::backend::Backend;
use database_mcp::server::Server;
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::process::ExitCode;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use clap::{Parser, Subcommand};

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
struct Cli {
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
        global = true
    )]
    log_level: LogLevel,
}

#[derive(Subcommand)]
enum Command {
    /// Print version information and exit
    Version,
    /// Run in stdio mode (default)
    Stdio,
    /// Run in HTTP/SSE mode
    Http {
        /// Bind host for HTTP transport
        #[arg(long, default_value = HttpConfig::DEFAULT_HOST)]
        host: String,

        /// Bind port for HTTP transport
        #[arg(long, default_value_t = HttpConfig::DEFAULT_PORT)]
        port: u16,

        /// Allowed CORS origins (comma-separated)
        #[arg(
            long = "allowed-origins",
            value_delimiter = ',',
            default_values_t = HttpConfig::default_allowed_origins()
        )]
        allowed_origins: Vec<String>,

        /// Allowed host names (comma-separated)
        #[arg(
            long = "allowed-hosts",
            value_delimiter = ',',
            default_values_t = HttpConfig::default_allowed_hosts()
        )]
        allowed_hosts: Vec<String>,
    },
}

impl From<&Cli> for DatabaseConfig {
    fn from(cli: &Cli) -> Self {
        let backend = cli.db_backend;
        Self {
            backend,
            host: cli.db_host.clone(),
            port: cli.db_port.unwrap_or_else(|| backend.default_port()),
            user: cli.db_user.clone().unwrap_or_else(|| backend.default_user().into()),
            password: cli.db_password.clone(),
            name: cli.db_name.clone(),
            charset: cli.db_charset.clone(),
            ssl: cli.db_ssl,
            ssl_ca: cli.db_ssl_ca.clone(),
            ssl_cert: cli.db_ssl_cert.clone(),
            ssl_key: cli.db_ssl_key.clone(),
            ssl_verify_cert: cli.db_ssl_verify_cert,
            read_only: cli.read_only,
            max_pool_size: cli.max_pool_size,
        }
    }
}

impl From<&Command> for Option<HttpConfig> {
    fn from(cmd: &Command) -> Self {
        if let Command::Http {
            host,
            port,
            allowed_origins,
            allowed_hosts,
        } = cmd
        {
            Some(HttpConfig {
                host: host.clone(),
                port: *port,
                allowed_origins: allowed_origins.clone(),
                allowed_hosts: allowed_hosts.clone(),
            })
        } else {
            None
        }
    }
}

impl From<&Cli> for Config {
    fn from(cli: &Cli) -> Self {
        Self {
            database: DatabaseConfig::from(cli),
            server: cli.command.as_ref().and_then(Into::into),
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
pub async fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if matches!(cli.command, Some(Command::Version)) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from(cli.log_level))
        .with_ansi(false)
        .init();

    let config = Config::from(&cli);
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

    let backend: Backend = match config.database.backend {
        DatabaseBackend::Sqlite => Backend::Sqlite(db::sqlite::SqliteBackend::new(&config.database).await?),
        DatabaseBackend::Postgres => Backend::Postgres(db::postgres::PostgresBackend::new(&config.database).await?),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => {
            Backend::Mysql(db::mysql::MysqlBackend::new(&config.database).await?)
        }
    };

    match cli.command {
        None | Some(Command::Stdio) => run_stdio(Server::new(backend)).await?,
        Some(Command::Http { .. }) => {
            let server = config.server.as_ref().expect("server config is set for HTTP command");
            run_http(backend, server).await?;
        }
        Some(Command::Version) => unreachable!("handled before backend initialization"),
    }

    Ok(ExitCode::SUCCESS)
}

async fn run_stdio(server: Server) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting MCP server via stdio transport...");

    let transport = rmcp::transport::io::stdio();
    let running = server.serve(transport).await?;

    running.waiting().await.ok();
    Ok(())
}

async fn run_http(backend: Backend, config: &HttpConfig) -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = format!("{}:{}", config.host, config.port);
    info!("Starting MCP server via HTTP transport on {bind_addr}...");

    let ct = CancellationToken::new();

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(
            config
                .allowed_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect::<Vec<axum::http::HeaderValue>>(),
        )
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::ACCEPT]);

    let service = StreamableHttpService::new(
        move || Ok(Server::new(backend.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig {
            stateful_mode: false,
            json_response: true,
            cancellation_token: ct.child_token(),
            ..Default::default()
        },
    );

    let router = axum::Router::new().nest_service("/mcp", service).layer(cors);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Listening on http://{bind_addr}/mcp");

    let ct_shutdown = ct.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Ctrl-C received, shutting down...");
        ct_shutdown.cancel();
    });

    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_db_backend_after_http_subcommand() {
        let cli = Cli::try_parse_from(["database-mcp", "http", "--db-backend", "mysql"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(cli.command, Some(Command::Http { .. })));
    }

    #[test]
    fn parse_db_backend_before_http_subcommand() {
        let cli = Cli::try_parse_from(["database-mcp", "--db-backend", "mysql", "http"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(cli.command, Some(Command::Http { .. })));
    }

    #[test]
    fn parse_db_backend_with_no_subcommand() {
        let cli = Cli::try_parse_from(["database-mcp", "--db-backend", "postgres"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Postgres);
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_multiple_global_args_after_subcommand() {
        let cli = Cli::try_parse_from([
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
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
        assert_eq!(cli.db_user, Some("root".into()));
        assert_eq!(cli.db_name, Some("mydb".into()));
    }

    #[test]
    fn parse_db_backend_defaults_to_mysql() {
        let cli = Cli::try_parse_from(["database-mcp", "http"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
    }

    #[test]
    fn cli_flag_overrides_default_backend() {
        let cli = Cli::try_parse_from(["database-mcp", "http", "--db-backend", "postgres"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Postgres);
    }

    #[test]
    fn parse_valid_log_levels() {
        for level in ["error", "warn", "info", "debug", "trace"] {
            let cli = Cli::try_parse_from(["database-mcp", "--log-level", level]).unwrap();
            assert_eq!(cli.log_level.to_string(), level);
        }
    }

    #[test]
    fn parse_invalid_log_level_is_rejected() {
        assert!(Cli::try_parse_from(["database-mcp", "--log-level", "nonsense"]).is_err());
    }

    #[test]
    fn log_level_defaults_to_info() {
        let cli = Cli::try_parse_from(["database-mcp"]).unwrap();
        assert_eq!(cli.log_level, LogLevel::Info);
    }

    #[test]
    fn parse_log_level_case_insensitive() {
        for level in ["DEBUG", "Info", "TRACE", "Warn", "ERROR"] {
            assert!(
                Cli::try_parse_from(["database-mcp", "--log-level", level]).is_ok(),
                "expected '{level}' to be accepted case-insensitively"
            );
        }
    }

    #[test]
    fn version_flag_is_accepted() {
        let result = Cli::try_parse_from(["database-mcp", "--version"]);
        // clap exits early for --version, so try_parse_from returns an Err
        // with DisplayVersion kind — not a "real" error.
        let err = result.err().expect("--version should cause clap to return Err");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn short_version_flag_is_accepted() {
        let err = Cli::try_parse_from(["database-mcp", "-V"])
            .err()
            .expect("-V should cause clap to return Err");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn version_subcommand_is_parsed() {
        let cli = Cli::try_parse_from(["database-mcp", "version"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version)));
    }
}
