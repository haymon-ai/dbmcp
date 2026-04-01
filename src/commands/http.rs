//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support.
//! Each HTTP session clones the pre-built handler, sharing the
//! underlying connection pools.

use clap::Parser;
use config::{Config, HttpConfig};
use rmcp::ServerHandler;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::root::RunError;

/// Runs the MCP server in HTTP mode.
#[derive(Debug, Parser)]
pub struct HttpCommand {
    /// Bind host for HTTP transport.
    #[arg(long, env = "HTTP_HOST", default_value = HttpConfig::DEFAULT_HOST)]
    pub host: String,

    /// Bind port for HTTP transport.
    #[arg(long, env = "HTTP_PORT", default_value_t = HttpConfig::DEFAULT_PORT)]
    pub port: u16,

    /// Allowed CORS origins (comma-separated).
    #[arg(
        long = "allowed-origins",
        env = "HTTP_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_origins()
    )]
    pub allowed_origins: Vec<String>,

    /// Allowed host names (comma-separated).
    #[arg(
        long = "allowed-hosts",
        env = "HTTP_ALLOWED_HOSTS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_hosts()
    )]
    pub allowed_hosts: Vec<String>,
}

impl HttpCommand {
    /// Starts the MCP server using HTTP transport.
    ///
    /// Binds to the configured host/port and serves MCP requests over
    /// Streamable HTTP. Each session clones the provided handler,
    /// sharing the underlying database connection pools. Supports CORS
    /// and graceful shutdown via Ctrl-C.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - HTTP config is missing from the configuration.
    /// - TCP bind fails (port in use, permission denied).
    pub async fn execute(
        &self,
        config: &Config,
        handler: impl ServerHandler + Clone + 'static,
    ) -> Result<(), RunError> {
        let http_config = config
            .http
            .as_ref()
            .ok_or_else(|| RunError::Config("HTTP configuration is missing".into()))?;
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
            move || Ok(handler.clone()),
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

        Ok(())
    }
}
