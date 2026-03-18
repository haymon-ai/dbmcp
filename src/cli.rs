//! CLI argument parsing and application bootstrapping.
//!
//! This module owns the entire bootstrapping pipeline: CLI argument parsing
//! (via clap with subcommands), tracing initialization, configuration
//! construction, validation, database backend creation, and MCP transport
//! dispatch.
//!
//! The binary has two subcommands:
//! - `stdio` (default) — runs the MCP server over stdin/stdout
//! - `http` — runs the MCP server over HTTP with Streamable HTTP transport

use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use sql_mcp::config::{Config, DatabaseBackend};
use sql_mcp::db;
use sql_mcp::db::backend::Backend;
use sql_mcp::server::Server;
use std::process::ExitCode;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sql-mcp", about = "Database MCP Server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", default_value_t = Config::DEFAULT_DB_BACKEND, global = true)]
    db_backend: DatabaseBackend,

    /// Database host
    #[arg(long = "db-host", env = "DB_HOST", default_value = Config::DEFAULT_DB_HOST, global = true)]
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
        default_value_t = Config::DEFAULT_DB_SSL,
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
        default_value_t = Config::DEFAULT_DB_SSL_VERIFY_CERT,
        global = true
    )]
    db_ssl_verify_cert: bool,

    /// Enable read-only mode
    #[arg(
        long = "read-only",
        env = "MCP_READ_ONLY",
        default_value_t = Config::DEFAULT_DB_READ_ONLY,
        global = true
    )]
    read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "max-pool-size",
        env = "MCP_MAX_POOL_SIZE",
        default_value_t = Config::DEFAULT_DB_MAX_POOL_SIZE,
        global = true,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    max_pool_size: u32,

    /// Log level (e.g. info, debug, warn)
    #[arg(
        long = "log-level",
        env = "LOG_LEVEL",
        default_value = Config::DEFAULT_LOG_LEVEL,
        global = true
    )]
    log_level: String,

    /// Log file path
    #[arg(
        long = "log-file",
        env = "LOG_FILE",
        default_value = Config::DEFAULT_LOG_FILE,
        global = true
    )]
    log_file: String,
}

#[derive(Subcommand)]
enum Command {
    /// Run in stdio mode (default)
    Stdio,
    /// Run in HTTP/SSE mode
    Http {
        /// Bind host for HTTP transport
        #[arg(long, default_value = Config::DEFAULT_HTTP_HOST)]
        host: String,

        /// Bind port for HTTP transport
        #[arg(long, default_value_t = Config::DEFAULT_HTTP_PORT)]
        port: u16,

        /// Allowed CORS origins (comma-separated)
        #[arg(
            long = "allowed-origins",
            value_delimiter = ',',
            default_values_t = Config::DEFAULT_HTTP_ALLOWED_ORIGINS.iter().map(|&s| s.to_string())
        )]
        allowed_origins: Vec<String>,

        /// Allowed host names (comma-separated)
        #[arg(
            long = "allowed-hosts",
            value_delimiter = ',',
            default_values_t = Config::DEFAULT_HTTP_ALLOWED_HOSTS.iter().map(|&s| s.to_string())
        )]
        allowed_hosts: Vec<String>,
    },
}

impl From<&Cli> for Config {
    fn from(cli: &Cli) -> Self {
        let backend = cli.db_backend;

        let mut config = Self {
            db_backend: backend,
            db_host: cli.db_host.clone(),
            db_port: cli.db_port.unwrap_or_else(|| backend.default_port()),
            db_user: cli
                .db_user
                .clone()
                .unwrap_or_else(|| backend.default_user().into()),
            db_password: cli.db_password.clone().unwrap_or_default(),
            db_name: cli.db_name.clone().unwrap_or_default(),
            db_charset: cli.db_charset.clone(),
            db_ssl: cli.db_ssl,
            db_ssl_ca: cli.db_ssl_ca.clone(),
            db_ssl_cert: cli.db_ssl_cert.clone(),
            db_ssl_key: cli.db_ssl_key.clone(),
            db_ssl_verify_cert: cli.db_ssl_verify_cert,
            db_read_only: cli.read_only,
            db_max_pool_size: cli.max_pool_size,
            log_level: cli.log_level.clone(),
            log_file: cli.log_file.clone(),
            http_host: Config::DEFAULT_HTTP_HOST.into(),
            http_port: Config::DEFAULT_HTTP_PORT,
            http_allowed_origins: Config::DEFAULT_HTTP_ALLOWED_ORIGINS
                .iter()
                .map(|&s| s.into())
                .collect(),
            http_allowed_hosts: Config::DEFAULT_HTTP_ALLOWED_HOSTS
                .iter()
                .map(|&s| s.into())
                .collect(),
        };

        if let Some(Command::Http {
            host,
            port,
            allowed_origins,
            allowed_hosts,
        }) = &cli.command
        {
            config.http_host.clone_from(host);
            config.http_port = *port;
            config.http_allowed_origins.clone_from(allowed_origins);
            config.http_allowed_hosts.clone_from(allowed_hosts);
        }

        config
    }
}

/// Parses CLI arguments, initialises the application, and runs the MCP server.
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

    let env_filter = tracing_subscriber::EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let log_path = std::path::Path::new(&cli.log_file);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or(std::path::Path::new(".")),
        log_path
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("mcp_server.log")),
    );

    tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_env_filter(env_filter)
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

    if config.db_read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    let backend: Backend = match config.db_backend {
        DatabaseBackend::Sqlite => Backend::Sqlite(db::sqlite::SqliteBackend::new(&config).await?),
        DatabaseBackend::Postgres => {
            Backend::Postgres(db::postgres::PostgresBackend::new(&config).await?)
        }
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => {
            Backend::Mysql(db::mysql::MysqlBackend::new(&config).await?)
        }
    };

    match cli.command {
        None | Some(Command::Stdio) => run_stdio(Server::new(backend)).await?,
        Some(Command::Http { .. }) => run_http(backend, &config).await?,
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

async fn run_http(backend: Backend, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = format!("{}:{}", config.http_host, config.http_port);
    info!("Starting MCP server via HTTP transport on {bind_addr}...");

    let ct = CancellationToken::new();

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(
            config
                .http_allowed_origins
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

    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(cors);

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
        let cli = Cli::try_parse_from(["sql-mcp", "http", "--db-backend", "mysql"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(cli.command, Some(Command::Http { .. })));
    }

    #[test]
    fn parse_db_backend_before_http_subcommand() {
        let cli = Cli::try_parse_from(["sql-mcp", "--db-backend", "mysql", "http"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
        assert!(matches!(cli.command, Some(Command::Http { .. })));
    }

    #[test]
    fn parse_db_backend_with_no_subcommand() {
        let cli = Cli::try_parse_from(["sql-mcp", "--db-backend", "postgres"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Postgres);
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_multiple_global_args_after_subcommand() {
        let cli = Cli::try_parse_from([
            "sql-mcp",
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
        let cli = Cli::try_parse_from(["sql-mcp", "http"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Mysql);
    }

    #[test]
    fn cli_flag_overrides_default_backend() {
        let cli = Cli::try_parse_from(["sql-mcp", "http", "--db-backend", "postgres"]).unwrap();
        assert_eq!(cli.db_backend, DatabaseBackend::Postgres);
    }
}
