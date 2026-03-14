//! Database MCP Server — a single-binary MCP server for multiple databases.
//!
//! Provides 6 database tools via the Model Context Protocol (MCP):
//! `list_databases`, `list_tables`, `get_table_schema`,
//! `get_table_schema_with_relations`, `execute_sql`, and `create_database`.
//!
//! Supports MySQL/MariaDB, `PostgreSQL`, and `SQLite` backends via `--database-type`.
//! Supports stdio and HTTP transport modes. Read-only mode is enabled by
//! default, enforcing AST-based SQL validation to block write operations.
//!
//! # Usage
//!
//! ```bash
//! # MySQL (default)
//! db-mcp
//!
//! # PostgreSQL
//! db-mcp --database-type postgres
//!
//! # SQLite
//! db-mcp --database-type sqlite --db-path ./data.db
//!
//! # HTTP mode
//! db-mcp --transport http --port 9001
//! ```

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use sql_mcp::config::Config;
use sql_mcp::db;
use sql_mcp::db::DatabaseType;
use sql_mcp::db::backend::Backend;
use sql_mcp::server::Server;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "db-mcp", about = "Database MCP Server")]
struct Cli {
    /// Transport mode
    #[arg(long, default_value = "stdio")]
    transport: Transport,

    /// Database type
    #[arg(long, default_value = "mysql")]
    database_type: DatabaseType,

    /// `SQLite` database file path
    #[arg(long)]
    db_path: Option<String>,

    /// Bind host for HTTP transport
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Bind port for HTTP transport
    #[arg(long, default_value_t = 9001)]
    port: u16,

    // -- Database connection --
    /// Database host address
    #[arg(long = "db-host")]
    db_host: Option<String>,

    /// Database port number
    #[arg(long = "db-port")]
    db_port: Option<u16>,

    /// Database user name
    #[arg(long = "db-user")]
    db_user: Option<String>,

    /// Database password
    #[arg(long = "db-password")]
    db_password: Option<String>,

    /// Default database name
    #[arg(long = "db-name")]
    db_name: Option<String>,

    /// Connection character set
    #[arg(long = "db-charset")]
    db_charset: Option<String>,

    // -- SSL/TLS --
    /// Enable SSL
    #[arg(long = "db-ssl")]
    db_ssl: Option<bool>,

    /// Path to CA certificate file
    #[arg(long = "db-ssl-ca")]
    db_ssl_ca: Option<String>,

    /// Path to client certificate file
    #[arg(long = "db-ssl-cert")]
    db_ssl_cert: Option<String>,

    /// Path to client private key file
    #[arg(long = "db-ssl-key")]
    db_ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(long = "db-ssl-verify-cert")]
    db_ssl_verify_cert: Option<bool>,

    /// Verify server hostname
    #[arg(long = "db-ssl-verify-identity")]
    db_ssl_verify_identity: Option<bool>,

    // -- MCP behavior --
    /// Enable read-only mode
    #[arg(long = "read-only")]
    read_only: Option<bool>,

    /// Maximum connection pool size
    #[arg(long = "max-pool-size")]
    max_pool_size: Option<u32>,

    // -- Network/CORS --
    /// Allowed CORS origins (comma-separated)
    #[arg(long = "allowed-origins", value_delimiter = ',')]
    allowed_origins: Option<Vec<String>>,

    /// Allowed host names (comma-separated)
    #[arg(long = "allowed-hosts", value_delimiter = ',')]
    allowed_hosts: Option<Vec<String>>,

    // -- Logging --
    /// Log level (e.g. info, debug, warn)
    #[arg(long = "log-level")]
    log_level: Option<String>,

    /// Log file path
    #[arg(long = "log-file")]
    log_file: Option<String>,

    /// Maximum log file size in bytes
    #[arg(long = "log-max-bytes")]
    log_max_bytes: Option<u64>,

    /// Number of rotated log backups to keep
    #[arg(long = "log-backup-count")]
    log_backup_count: Option<u32>,
}

#[derive(Clone, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[tokio::main]
async fn main() {
    // Load .env file (ignore if missing)
    dotenvy::dotenv().ok();

    // Parse CLI args
    let cli = Cli::parse();

    // Initialize tracing
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("LOG_LEVEL")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let log_file = std::env::var("LOG_FILE").unwrap_or_else(|_| "logs/mcp_server.log".into());
    let log_path = std::path::Path::new(&log_file);
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

    // Load configuration from env, then apply CLI overrides
    let mut config = match Config::from_env_without_validation() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(1);
        }
    };
    apply_cli_overrides(&mut config, &cli);
    if let Err(e) = config.validate() {
        eprintln!("Configuration error: {e}");
        std::process::exit(1);
    }

    if config.mcp.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    // Create the appropriate database backend
    let backend: Backend = match cli.database_type {
        DatabaseType::Mysql => match db::mysql::MysqlBackend::new(&config).await {
            Ok(b) => Backend::Mysql(b),
            Err(e) => {
                eprintln!("Failed to connect to MySQL: {e}");
                std::process::exit(1);
            }
        },
        DatabaseType::Postgres => match db::postgres::PostgresBackend::new(&config).await {
            Ok(b) => Backend::Postgres(b),
            Err(e) => {
                eprintln!("Failed to connect to PostgreSQL: {e}");
                std::process::exit(1);
            }
        },
        DatabaseType::Sqlite => {
            let db_path = cli.db_path.as_deref().unwrap_or_else(|| {
                eprintln!("SQLite requires --db-path flag");
                std::process::exit(1);
            });
            match db::sqlite::SqliteBackend::new(db_path, config.mcp.read_only).await {
                Ok(b) => Backend::Sqlite(b),
                Err(e) => {
                    eprintln!("Failed to open SQLite: {e}");
                    std::process::exit(1);
                }
            }
        }
    };

    let config = Arc::new(config);

    match cli.transport {
        Transport::Stdio => run_stdio(Server::new(backend)).await,
        Transport::Http => run_http(backend, config, &cli.host, cli.port).await,
    }
}

async fn run_stdio(server: Server) {
    info!("Starting MCP server via stdio transport...");

    let transport = rmcp::transport::io::stdio();
    let running = match server.serve(transport).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to start MCP server: {e}");
            std::process::exit(1);
        }
    };

    running.waiting().await.ok();
}

async fn run_http(backend: Backend, config: Arc<Config>, host: &str, port: u16) {
    let bind_addr = format!("{host}:{port}");
    info!("Starting MCP server via HTTP transport on {bind_addr}...");

    let ct = CancellationToken::new();

    let allowed_origins = config.network.allowed_origins.clone();
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

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => {
            info!("Listening on http://{bind_addr}/mcp");
            l
        }
        Err(e) => {
            eprintln!("Failed to bind to {bind_addr}: {e}");
            std::process::exit(1);
        }
    };

    let ct_shutdown = ct.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Ctrl-C received, shutting down...");
        ct_shutdown.cancel();
    });

    if let Err(e) = axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await
    {
        eprintln!("HTTP server error: {e}");
        std::process::exit(1);
    }
}

/// Applies CLI argument overrides onto an env-loaded [`Config`].
///
/// Only fields explicitly provided via CLI (i.e. `Some(value)`) override.
fn apply_cli_overrides(config: &mut Config, cli: &Cli) {
    if let Some(v) = &cli.db_host {
        config.database.host.clone_from(v);
    }
    if let Some(v) = cli.db_port {
        config.database.port = v;
    }
    if let Some(v) = &cli.db_user {
        config.database.user.clone_from(v);
    }
    if let Some(v) = &cli.db_password {
        config.database.password.clone_from(v);
    }
    if cli.db_name.is_some() {
        config.database.name.clone_from(&cli.db_name);
    }
    if cli.db_charset.is_some() {
        config.database.charset.clone_from(&cli.db_charset);
    }
    if let Some(v) = cli.db_ssl {
        config.ssl.enabled = v;
    }
    if cli.db_ssl_ca.is_some() {
        config.ssl.ca.clone_from(&cli.db_ssl_ca);
    }
    if cli.db_ssl_cert.is_some() {
        config.ssl.cert.clone_from(&cli.db_ssl_cert);
    }
    if cli.db_ssl_key.is_some() {
        config.ssl.key.clone_from(&cli.db_ssl_key);
    }
    if let Some(v) = cli.db_ssl_verify_cert {
        config.ssl.verify_cert = v;
    }
    if let Some(v) = cli.db_ssl_verify_identity {
        config.ssl.verify_identity = v;
    }
    if let Some(v) = cli.read_only {
        config.mcp.read_only = v;
    }
    if let Some(v) = cli.max_pool_size {
        config.mcp.max_pool_size = v;
    }
    if let Some(v) = &cli.allowed_origins {
        config.network.allowed_origins.clone_from(v);
    }
    if let Some(v) = &cli.allowed_hosts {
        config.network.allowed_hosts.clone_from(v);
    }
    if let Some(v) = &cli.log_level {
        config.log.level.clone_from(v);
    }
    if let Some(v) = &cli.log_file {
        config.log.file.clone_from(v);
    }
    if let Some(v) = cli.log_max_bytes {
        config.log.max_bytes = v;
    }
    if let Some(v) = cli.log_backup_count {
        config.log.backup_count = v;
    }
}
