//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support.
//! Each HTTP session clones the pre-built handler, sharing the
//! underlying connection pools.

use clap::{Args, Parser};
use database_mcp_config::{ConfigError, DatabaseConfig, HttpConfig};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::process::ExitCode;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::commands::common::{self, DatabaseArguments};
use crate::error::Error;

/// HTTP transport flags embedded in [`HttpCommand`].
#[derive(Debug, Args)]
struct HttpArguments {
    /// Bind host for HTTP transport.
    #[arg(id = "http-host", long = "host", env = "HTTP_HOST", default_value = HttpConfig::DEFAULT_HOST)]
    host: String,

    /// Bind port for HTTP transport.
    #[arg(id = "http-port", long = "port", env = "HTTP_PORT", default_value_t = HttpConfig::DEFAULT_PORT)]
    port: u16,

    /// Allowed CORS origins (comma-separated).
    #[arg(
        long = "allowed-origins",
        env = "HTTP_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_origins()
    )]
    allowed_origins: Vec<String>,

    /// Allowed host names (comma-separated).
    #[arg(
        long = "allowed-hosts",
        env = "HTTP_ALLOWED_HOSTS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_hosts()
    )]
    allowed_hosts: Vec<String>,
}

impl TryFrom<&HttpArguments> for HttpConfig {
    type Error = Vec<ConfigError>;

    fn try_from(http: &HttpArguments) -> Result<Self, Self::Error> {
        let config = Self {
            host: http.host.clone(),
            port: http.port,
            allowed_origins: http.allowed_origins.clone(),
            allowed_hosts: http.allowed_hosts.clone(),
        };
        config.validate()?;
        Ok(config)
    }
}

/// Runs the MCP server in HTTP mode.
#[derive(Debug, Parser)]
pub(crate) struct HttpCommand {
    /// Shared database connection flags.
    #[command(flatten)]
    pub(crate) db_arguments: DatabaseArguments,

    /// HTTP transport flags.
    #[command(flatten)]
    http_arguments: HttpArguments,
}

impl HttpCommand {
    /// Builds the database configuration, server, and runs the HTTP transport.
    ///
    /// Binds to the configured host/port and serves MCP requests over
    /// Streamable HTTP. Each session clones the internally-built handler,
    /// sharing the underlying database connection pools. Supports CORS
    /// and graceful shutdown via Ctrl-C. Returns [`ExitCode::FAILURE`]
    /// when configuration validation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if TCP bind fails (port in use, permission
    /// denied) or the HTTP service fails to serve.
    pub(crate) async fn execute(&self) -> Result<ExitCode, Error> {
        let db_config = DatabaseConfig::try_from(&self.db_arguments)?;
        let server = common::create_server(&db_config);

        let http_config = HttpConfig::try_from(&self.http_arguments)?;
        let bind_addr = format!("{}:{}", http_config.host, http_config.port);
        info!("Starting MCP server via HTTP transport on {bind_addr}...");

        let ct = CancellationToken::new();

        let cors = tower_http::cors::CorsLayer::new()
            .allow_origin(
                http_config
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
            move || Ok(server.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true)
                .with_cancellation_token(ct.child_token()),
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

        Ok(ExitCode::SUCCESS)
    }
}
