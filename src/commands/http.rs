//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support.
//! Each HTTP session clones the internally-built handler, sharing
//! the underlying connection pools.

use clap::{Args, Parser};
use dbmcp_config::{Config, ConfigError, ConfigErrors, DatabaseConfig, HttpConfig, PiiConfig};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::commands::common::{self, DatabaseArguments, PiiArguments};
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
        default_value = HttpConfig::DEFAULT_HOST,
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
    type Error = ConfigErrors;

    fn try_from(http: &HttpArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            host: http.host.clone(),
            port: http.port,
            allowed_origins: http.allowed_origins.clone(),
            allowed_hosts: http.allowed_hosts.clone(),
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Runs the MCP server in HTTP mode.
#[derive(Debug, Parser)]
pub(crate) struct HttpCommand {
    /// Shared database connection flags.
    #[command(flatten)]
    database: DatabaseArguments,

    /// HTTP transport flags.
    #[command(flatten)]
    http: HttpArguments,

    /// Shared PII flags.
    #[command(flatten)]
    pii: PiiArguments,
}

impl TryFrom<&HttpCommand> for Config {
    type Error = ConfigErrors;

    fn try_from(cmd: &HttpCommand) -> Result<Self, Self::Error> {
        match (
            DatabaseConfig::try_from(&cmd.database),
            HttpConfig::try_from(&cmd.http),
            PiiConfig::try_from(&cmd.pii),
        ) {
            (Ok(database), Ok(http), Ok(pii)) => Ok(Self {
                database,
                http: Some(http),
                pii,
            }),
            (database, http, pii) => {
                let mut errors: Vec<ConfigError> = Vec::new();
                if let Err(e) = database {
                    errors.extend(e);
                }
                if let Err(e) = http {
                    errors.extend(e);
                }
                if let Err(e) = pii {
                    errors.extend(e);
                }
                Err(ConfigErrors::from_vec(errors).expect("non-Ok branch implies at least one Err"))
            }
        }
    }
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
        let config = Config::try_from(self)?;
        let http_config = config.http.as_ref().expect("http config set by TryFrom impl");

        let server = common::create_server(&config);
        let cancel_token = CancellationToken::new();

        let router = build_http_router(http_config, server, &cancel_token);

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

    #[derive(Debug, Parser)]
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
        let config = Config {
            database: sqlite_memory_db_config(),
            http: Some(http_config.clone()),
            pii: PiiConfig::default(),
        };
        let server = common::create_server(&config);
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

    fn clear_http_env() {
        // SAFETY: tests in this file do not write HTTP_* env vars concurrently.
        unsafe {
            std::env::remove_var("HTTP_HOST");
            std::env::remove_var("HTTP_PORT");
            std::env::remove_var("HTTP_ALLOWED_ORIGINS");
            std::env::remove_var("HTTP_ALLOWED_HOSTS");
        }
    }

    #[test]
    fn http_config_validate_is_called_from_try_from_path() {
        // Structural-presence guard: ensures HttpConfig::try_from invokes
        // HttpConfig::validate. When a future rule fires on default args,
        // this test deliberately fails — flagging the contributor.
        let args = parse_http_args(&[]);
        HttpConfig::try_from(&args).expect("default http args must validate");
    }

    #[test]
    fn top_level_try_from_http_accumulates_only_database_errors_today() {
        // HttpConfig::validate and PiiConfig::validate are no-ops today, so
        // only DatabaseConfig errors propagate. When http or pii gain a rule
        // that fires on default args, widen the assertion to verify
        // db→http→pii ordering (FR-006).
        clear_http_env();
        let cmd = HttpCommand::try_parse_from(["_", "--db-backend", "sqlite"]).expect("clap parse");
        let errors = Config::try_from(&cmd).expect_err("sqlite without name must fail");
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
    }

    #[test]
    fn clap_default_yields_four_loopback_origins() {
        let args = parse_http_args(&[]);
        let config = HttpConfig::try_from(&args).expect("default args are valid");
        assert_eq!(config.allowed_origins, HttpConfig::default_allowed_origins());
    }

    #[test]
    fn try_from_rejects_empty_host() {
        clear_http_env();
        let cli = TestCli::try_parse_from(["--host", ""]).expect("clap must accept empty host");
        let errors = HttpConfig::try_from(&cli.http).expect_err("empty host must be rejected by validate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn try_from_rejects_whitespace_host() {
        clear_http_env();
        let cli = TestCli::try_parse_from(["--host", "   "]).expect("clap must accept whitespace host");
        let errors = HttpConfig::try_from(&cli.http).expect_err("whitespace host must be rejected by validate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn clap_accepts_default_host() {
        clear_http_env();
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).expect("clap parse");
        assert_eq!(cli.http.host, HttpConfig::DEFAULT_HOST);
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
