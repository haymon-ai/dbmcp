//! Database MCP Server — a single-binary MCP server for multiple databases.
//!
//! Provides 6 database tools via the Model Context Protocol (MCP):
//! `list_databases`, `list_tables`, `get_table_schema`,
//! `get_table_schema_with_relations`, `execute_sql`, and `create_database`.
//!
//! Supports MySQL/MariaDB, `PostgreSQL`, and `SQLite` backends.
//! Supports stdio and HTTP transport modes. Read-only mode is enabled by
//! default, enforcing AST-based SQL validation to block write operations.
//!
//! # Usage
//!
//! ```bash
//! # MySQL (default)
//! db-mcp --database-url mysql://root@localhost/mydb
//!
//! # PostgreSQL
//! db-mcp --database-url postgres://user@localhost:5432/mydb
//!
//! # SQLite
//! db-mcp --database-url sqlite:./data.db
//!
//! # HTTP mode
//! db-mcp --database-url mysql://root@localhost/mydb --transport http --port 9001
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

    /// Database connection URL in sqlx DSN format
    #[arg(long = "database-url")]
    database_url: String,

    /// Bind host for HTTP transport
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Bind port for HTTP transport
    #[arg(long, default_value_t = 9001)]
    port: u16,

    // -- MCP behavior --
    /// Enable read-only mode
    #[arg(long = "read-only", default_value_t = true)]
    read_only: bool,

    /// Maximum connection pool size
    #[arg(long = "max-pool-size", default_value_t = 10)]
    max_pool_size: u32,

    // -- Network/CORS --
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

    // -- Logging --
    /// Log level (e.g. info, debug, warn)
    #[arg(long = "log-level", default_value = "info")]
    log_level: String,

    /// Log file path
    #[arg(long = "log-file", default_value = "logs/mcp_server.log")]
    log_file: String,

    /// Maximum log file size in bytes
    #[arg(long = "log-max-bytes", default_value_t = 10_485_760)]
    log_max_bytes: u64,

    /// Number of rotated log backups to keep
    #[arg(long = "log-backup-count", default_value_t = 5)]
    log_backup_count: u32,
}

impl From<Cli> for Config {
    fn from(cli: Cli) -> Self {
        Self {
            database_url: cli.database_url,
            read_only: cli.read_only,
            max_pool_size: cli.max_pool_size,
            allowed_origins: cli.allowed_origins,
            allowed_hosts: cli.allowed_hosts,
            log_level: cli.log_level,
            log_file: cli.log_file,
            log_max_bytes: cli.log_max_bytes,
            log_backup_count: cli.log_backup_count,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[tokio::main]
async fn main() {
    // Parse CLI args
    let cli = Cli::parse();

    // Extract transport and bind info before consuming cli
    let transport = cli.transport.clone();
    let host = cli.host.clone();
    let port = cli.port;

    // Initialize tracing using CLI-provided values
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

    // Build config from CLI args
    let config: Config = cli.into();

    if config.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    // Detect database type from URL scheme and create the appropriate backend
    let backend: Backend = if config.database_url.starts_with("sqlite:") {
        match db::sqlite::SqliteBackend::new(&config.database_url, config.read_only).await {
            Ok(b) => Backend::Sqlite(b),
            Err(e) => {
                eprintln!("Failed to open SQLite: {e}");
                std::process::exit(1);
            }
        }
    } else if config.database_url.starts_with("postgres://")
        || config.database_url.starts_with("postgresql://")
    {
        match db::postgres::PostgresBackend::new(&config).await {
            Ok(b) => Backend::Postgres(b),
            Err(e) => {
                eprintln!("Failed to connect to PostgreSQL: {e}");
                std::process::exit(1);
            }
        }
    } else {
        // Default: mysql:// or mariadb://
        match db::mysql::MysqlBackend::new(&config).await {
            Ok(b) => Backend::Mysql(b),
            Err(e) => {
                eprintln!("Failed to connect to MySQL: {e}");
                std::process::exit(1);
            }
        }
    };

    let config = Arc::new(config);

    match transport {
        Transport::Stdio => run_stdio(Server::new(backend)).await,
        Transport::Http => run_http(backend, config, &host, port).await,
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

    let allowed_origins = config.allowed_origins.clone();
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
