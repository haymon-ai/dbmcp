//! CLI argument parsing and application bootstrapping.
//!
//! This module owns the entire bootstrapping pipeline: `.env` file loading,
//! CLI argument parsing (via clap with subcommands), tracing initialization,
//! configuration construction, validation, database backend creation, and
//! MCP transport dispatch.
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
#[command(name = "db-mcp", about = "Database MCP Server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    // -- Database connection --
    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", global = true)]
    db_backend: DatabaseBackend,

    /// Database host
    #[arg(long = "db-host", env = "DB_HOST", global = true)]
    db_host: Option<String>,

    /// Database port
    #[arg(long = "db-port", env = "DB_PORT", global = true)]
    db_port: Option<u16>,

    /// Database user
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

    // -- SSL/TLS --
    /// Enable SSL for database connection
    #[arg(
        long = "db-ssl",
        env = "DB_SSL",
        default_value_t = false,
        global = true
    )]
    db_ssl: bool,

    /// Path to CA certificate
    #[arg(long = "db-ssl-ca", env = "DB_SSL_CA", global = true)]
    db_ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(long = "db-ssl-cert", env = "DB_SSL_CERT", global = true)]
    db_ssl_cert: Option<String>,

    /// Path to client key
    #[arg(long = "db-ssl-key", env = "DB_SSL_KEY", global = true)]
    db_ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(
        long = "db-ssl-verify-cert",
        env = "DB_SSL_VERIFY_CERT",
        default_value_t = true,
        global = true
    )]
    db_ssl_verify_cert: bool,

    // -- MCP behavior --
    /// Enable read-only mode
    #[arg(
        long = "read-only",
        env = "MCP_READ_ONLY",
        default_value_t = true,
        global = true
    )]
    read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "max-pool-size",
        env = "MCP_MAX_POOL_SIZE",
        default_value_t = 10,
        global = true,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    max_pool_size: u32,

    // -- Logging --
    /// Log level (e.g. info, debug, warn)
    #[arg(
        long = "log-level",
        env = "LOG_LEVEL",
        default_value = "info",
        global = true
    )]
    log_level: String,

    /// Log file path
    #[arg(
        long = "log-file",
        env = "LOG_FILE",
        default_value = "logs/mcp_server.log",
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
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Bind port for HTTP transport
        #[arg(long, default_value_t = 9001)]
        port: u16,

        /// Allowed CORS origins (comma-separated)
        #[arg(
            long = "allowed-origins",
            value_delimiter = ',',
            default_values_t = vec![
                "http://localhost".to_string(),
                "http://127.0.0.1".to_string(),
                "https://localhost".to_string(),
                "https://127.0.0.1".to_string(),
            ]
        )]
        allowed_origins: Vec<String>,

        /// Allowed host names (comma-separated)
        #[arg(
            long = "allowed-hosts",
            value_delimiter = ',',
            default_values_t = vec!["localhost".to_string(), "127.0.0.1".to_string()]
        )]
        allowed_hosts: Vec<String>,
    },
}

impl From<&Cli> for Config {
    fn from(cli: &Cli) -> Self {
        let mut config = Self {
            db_backend: cli.db_backend,
            db_host: cli.db_host.clone(),
            db_port: cli.db_port,
            db_user: cli.db_user.clone(),
            db_password: cli.db_password.clone(),
            db_name: cli.db_name.clone(),
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
            http_host: None,
            http_port: None,
            http_allowed_origins: None,
            http_allowed_hosts: None,
        };

        if let Some(Command::Http {
            host,
            port,
            allowed_origins,
            allowed_hosts,
        }) = &cli.command
        {
            config.http_host = Some(host.clone());
            config.http_port = Some(*port);
            config.http_allowed_origins = Some(allowed_origins.clone());
            config.http_allowed_hosts = Some(allowed_hosts.clone());
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
    let host = config
        .http_host
        .as_deref()
        .expect("http subcommand sets host");
    let port = config.http_port.expect("http subcommand sets port");
    let allowed_origins = config
        .http_allowed_origins
        .as_ref()
        .expect("http subcommand sets allowed_origins");

    let bind_addr = format!("{host}:{port}");
    info!("Starting MCP server via HTTP transport on {bind_addr}...");

    let ct = CancellationToken::new();

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(
            allowed_origins
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
