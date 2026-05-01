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

    /// Allowed browser origins (comma-separated, RFC 6454 `<scheme>://<host>[:<port>]` or `null`).
    ///
    /// Drives BOTH the CORS preflight allowlist AND the rmcp server-side
    /// Origin validator. Pass an empty list (`--allowed-origins ""`) to
    /// disable both layers — only do this for non-browser deployments.
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

        let router = build_http_router(&http_config, server, &cancel_token);

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

/// Builds the axum router that serves MCP over Streamable HTTP.
///
/// Wires the configured allowed-origins list into BOTH the rmcp 1.6.0
/// server-side Origin validator and the tower-http CORS preflight layer
/// so the two layers cannot disagree by accident.
fn build_http_router(
    http_config: &HttpConfig,
    server: common::Server,
    cancel_token: &CancellationToken,
) -> axum::Router {
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true)
            .with_cancellation_token(cancel_token.child_token())
            .with_allowed_hosts(http_config.allowed_hosts.clone())
            .with_allowed_origins(http_config.allowed_origins.clone()),
    );

    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(build_cors_layer(http_config))
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

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderValue, Method, Request, StatusCode, header, request::Builder};
    use clap::Parser;
    use dbmcp_config::DatabaseBackend;
    use tower::ServiceExt;

    #[derive(Parser)]
    #[command(no_binary_name = true)]
    struct TestCli {
        #[command(flatten)]
        http: HttpArguments,
    }

    fn parse_http_args(args: &[&str]) -> HttpArguments {
        // SAFETY: tests don't run concurrently against these env vars.
        unsafe {
            std::env::remove_var("HTTP_ALLOWED_ORIGINS");
            std::env::remove_var("HTTP_ALLOWED_HOSTS");
        }
        TestCli::try_parse_from(args).expect("clap parse").http
    }

    fn sqlite_memory_db_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some(":memory:".into()),
            ..DatabaseConfig::default()
        }
    }

    fn router_with_origins(origins: Vec<String>) -> axum::Router {
        let http_config = HttpConfig {
            host: HttpConfig::DEFAULT_HOST.into(),
            port: HttpConfig::DEFAULT_PORT,
            allowed_origins: origins,
            allowed_hosts: HttpConfig::default_allowed_hosts(),
        };
        let server = common::create_server(&sqlite_memory_db_config());
        let cancel = CancellationToken::new();
        build_http_router(&http_config, server, &cancel)
    }

    async fn send(router: axum::Router, request: Request<Body>) -> (StatusCode, String) {
        let response = router.oneshot(request).await.expect("oneshot");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("body bytes");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn mcp_post(uri: &str) -> Builder {
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
    }

    #[test]
    fn clap_default_yields_four_loopback_origins() {
        let args = parse_http_args(&[]);
        let config = HttpConfig::try_from(&args).expect("default config valid");
        assert_eq!(config.allowed_origins, HttpConfig::default_allowed_origins());
    }

    #[tokio::test]
    async fn disallowed_origin_returns_403() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = mcp_post("/mcp/")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
        assert_eq!(body, "Forbidden: Origin header is not allowed");
    }

    #[tokio::test]
    async fn disallowed_host_returns_403() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/mcp/")
            .header(header::HOST, "evil.example")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
        assert_eq!(body, "Forbidden: Host header is not allowed");
    }

    #[tokio::test]
    async fn malformed_origin_non_utf8_returns_400() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let mut request = mcp_post("/mcp/").body(Body::from("{}")).expect("build request");
        request.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_bytes(&[0xFF, 0xFE]).expect("non-utf8 header value"),
        );
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert_eq!(body, "Bad Request: Invalid Origin header encoding");
    }

    #[tokio::test]
    async fn malformed_origin_unparseable_returns_400() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = mcp_post("/mcp/")
            .header(header::ORIGIN, "not a url")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert_eq!(body, "Bad Request: Invalid Origin header");
    }

    #[tokio::test]
    async fn allowed_origin_passes_validator() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = mcp_post("/mcp/")
            .header(header::ORIGIN, "http://localhost")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, _body) = send(router, request).await;
        assert_ne!(status, StatusCode::FORBIDDEN);
        assert_ne!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn missing_origin_passes_validator() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = mcp_post("/mcp/").body(Body::from("{}")).expect("build request");
        let (status, _body) = send(router, request).await;
        assert_ne!(status, StatusCode::FORBIDDEN);
        assert_ne!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn empty_allowlist_skips_origin_check() {
        let router = router_with_origins(vec![]);
        let request = mcp_post("/mcp/")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, _body) = send(router, request).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "empty allowlist must not reject Origin");
    }
}
