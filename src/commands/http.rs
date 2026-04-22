//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support.
//! Each HTTP session clones the internally-built handler, sharing
//! the underlying connection pools.

use clap::{Args, Parser};
use dbmcp_config::{ConfigError, DatabaseConfig, HttpConfig};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::commands::common::{self, DatabaseArguments};
use crate::error::Error;

/// HTTP transport flags embedded in [`HttpCommand`].
///
/// `host` and `port` use explicit `id = "http-*"` overrides so their
/// clap argument ids don't collide with the `host`/`port` fields in
/// [`DatabaseArguments`] when both are flattened into [`HttpCommand`].
#[derive(Debug, Args)]
#[command(next_help_heading = "HTTP Transport")]
struct HttpArguments {
    /// Bind host for HTTP transport.
    #[arg(
        id = "http-host",
        long = "host",
        env = "HTTP_HOST",
        value_name = "HOST",
        default_value = HttpConfig::DEFAULT_HOST
    )]
    host: String,

    /// Bind port for HTTP transport.
    #[arg(
        id = "http-port",
        long = "port",
        env = "HTTP_PORT",
        value_name = "PORT",
        default_value_t = HttpConfig::DEFAULT_PORT
    )]
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
    db_arguments: DatabaseArguments,

    /// HTTP transport flags.
    #[command(flatten)]
    http_arguments: HttpArguments,
}

impl HttpCommand {
    /// Builds the database configuration, server, and runs the HTTP transport.
    ///
    /// Binds to the configured host/port and serves MCP requests over
    /// Streamable HTTP. Each session clones the internally-built handler,
    /// sharing the underlying database connection pools. Shuts down
    /// gracefully on Ctrl-C or `SIGTERM`.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation fails, TCP bind
    /// fails (port in use, permission denied), or the HTTP service
    /// fails to serve.
    pub(crate) async fn execute(&self) -> Result<(), Error> {
        let db_config = DatabaseConfig::try_from(&self.db_arguments)?;
        let http_config = HttpConfig::try_from(&self.http_arguments)?;

        let server = common::create_server(&db_config);
        let cancel_token = CancellationToken::new();

        let service = StreamableHttpService::new(
            move || Ok(server.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true)
                .with_cancellation_token(cancel_token.child_token()),
        );

        let router = axum::Router::new()
            .nest_service("/mcp", service)
            .layer(build_cors_layer(&http_config));

        let bind_addr = format!("{}:{}", http_config.host, http_config.port);
        info!("Starting MCP server via HTTP transport on {bind_addr}...");

        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        info!("Listening on http://{bind_addr}/mcp");

        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                cancel_token.cancel();
            })
            .await?;

        Ok(())
    }
}

/// Builds a CORS layer from the configured allowed origins.
fn build_cors_layer(http_config: &HttpConfig) -> CorsLayer {
    let origins: Vec<axum::http::HeaderValue> = http_config
        .allowed_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::ACCEPT])
}

/// Future that resolves when the process should shut down.
///
/// Listens for Ctrl-C on all platforms and `SIGTERM` on Unix, which
/// is the signal `docker stop`, `systemctl stop`, and Kubernetes
/// send to request graceful termination. Whichever arrives first
/// wins.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Ctrl-C received, shutting down..."),
        () = terminate => info!("SIGTERM received, shutting down..."),
    }
}
