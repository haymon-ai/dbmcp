//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support and
//! optional bearer-token authentication. Each HTTP session clones the
//! internally-built handler, sharing the underlying connection pools.

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use clap::{Args, Parser};
use dbmcp_config::{Config, ConfigError, ConfigErrors, DatabaseConfig, HttpConfig, PiiConfig};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::commands::common::{self, DatabaseArguments, PiiArguments};
use crate::error::Error;

/// HTTP transport flags embedded in [`HttpCommand`].
///
/// `host` and `port` use explicit `id = "http-*"` overrides so their
/// clap argument ids don't collide with the `host`/`port` fields in
/// [`DatabaseArguments`] when both are flattened into [`HttpCommand`].
#[derive(Args)]
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

    /// Bearer token clients MUST send as `Authorization: Bearer <token>`.
    ///
    /// Required when binding a non-loopback host. Prefer the env var:
    /// command-line arguments are visible to other local users.
    #[arg(
        long = "auth-token",
        env = "HTTP_AUTH_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    auth_token: Option<String>,

    /// Allow binding a non-loopback host without `HTTP_AUTH_TOKEN`.
    ///
    /// Only for deployments where a reverse proxy authenticates callers.
    /// Anyone who can reach the port gets full access to the database.
    #[arg(
        long = "dangerously-allow-unauthenticated",
        env = "HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED"
    )]
    dangerously_allow_unauthenticated: bool,
}

impl std::fmt::Debug for HttpArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpArguments")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("allowed_origins", &self.allowed_origins)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "[REDACTED]"))
            .field(
                "dangerously_allow_unauthenticated",
                &self.dangerously_allow_unauthenticated,
            )
            .finish()
    }
}

impl TryFrom<&HttpArguments> for HttpConfig {
    type Error = ConfigErrors;

    fn try_from(http: &HttpArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            host: http.host.clone(),
            port: http.port,
            allowed_origins: http.allowed_origins.clone(),
            allowed_hosts: http.allowed_hosts.clone(),
            auth_token: http.auth_token.clone(),
            allow_unauthenticated: http.dangerously_allow_unauthenticated,
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

        let server = common::create_server(&config)?;
        let cancel_token = CancellationToken::new();

        let router = build_http_router(http_config, server.clone(), &cancel_token);

        let bind_addr = format!("{}:{}", http_config.host, http_config.port);
        info!("Starting MCP server via HTTP transport on {bind_addr}...");

        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        info!("Listening on http://{bind_addr}/mcp");
        if http_config.auth_token.is_some() {
            info!("Bearer token authentication enabled");
        } else if http_config.is_loopback() {
            info!("Bearer token authentication disabled (loopback bind)");
        } else {
            warn!(
                "Bearer token authentication DISABLED on non-loopback bind {bind_addr}; \
                 anyone who can reach this port can access the database"
            );
        }

        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                common::shutdown_signal().await;
                cancel_token.cancel();
            })
            .await?;

        server.shutdown().await;
        info!("All connection pools closed.");

        Ok(())
    }
}

/// Builds the axum router that serves MCP over Streamable HTTP.
///
/// Wires the configured allowed-origins list into BOTH the rmcp
/// server-side Origin validator and the tower-http CORS preflight layer
/// so the two layers cannot disagree by accident. When an auth token is
/// configured, every request must pass [`require_bearer`] before it
/// reaches the MCP service. CORS stays outermost so browser preflights,
/// which never carry credentials, are answered without authentication.
fn build_http_router(
    http_config: &HttpConfig,
    server: common::Server,
    cancel_token: &CancellationToken,
) -> axum::Router {
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_cancellation_token(cancel_token.child_token())
            .with_allowed_hosts(http_config.allowed_hosts.clone())
            .with_allowed_origins(http_config.allowed_origins.clone()),
    );

    let mut router = axum::Router::new().nest_service("/mcp", service);
    if let Some(token) = &http_config.auth_token {
        router = router.layer(axum::middleware::from_fn_with_state(
            BearerAuth::new(token),
            require_bearer,
        ));
    }
    router.layer(build_cors_layer(http_config))
}

/// SHA-256 digest of the expected bearer token.
///
/// Comparing fixed-length digests in constant time hides both the token
/// contents and its length from timing side channels.
#[derive(Clone)]
struct BearerAuth(Arc<[u8]>);

impl BearerAuth {
    fn new(token: &str) -> Self {
        Self(Sha256::digest(token.trim().as_bytes()).to_vec().into())
    }

    fn matches(&self, presented: &str) -> bool {
        Sha256::digest(presented.as_bytes()).as_slice().ct_eq(&self.0).into()
    }
}

/// Rejects requests lacking a valid `Authorization: Bearer <token>` header.
async fn require_bearer(State(auth): State<BearerAuth>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim());

    if presented.is_some_and(|token| auth.matches(token)) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, r#"Bearer realm="dbmcp""#)],
            "Unauthorized",
        )
            .into_response()
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
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT, header::AUTHORIZATION])
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderValue, Method, Request, request::Builder};
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
            std::env::remove_var("HTTP_AUTH_TOKEN");
            std::env::remove_var("HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED");
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

    const TEST_TOKEN: &str = "test-token-123";

    fn router_with_origins(origins: Vec<String>) -> axum::Router {
        router_with(origins, None)
    }

    fn router_with_token() -> axum::Router {
        router_with(HttpConfig::default_allowed_origins(), Some(TEST_TOKEN.into()))
    }

    fn router_with(origins: Vec<String>, auth_token: Option<String>) -> axum::Router {
        let http_config = HttpConfig {
            host: HttpConfig::DEFAULT_HOST.into(),
            port: HttpConfig::DEFAULT_PORT,
            allowed_origins: origins,
            allowed_hosts: HttpConfig::default_allowed_hosts(),
            auth_token,
            allow_unauthenticated: false,
        };
        let config = Config {
            database: sqlite_memory_db_config(),
            http: Some(http_config.clone()),
            pii: PiiConfig::default(),
        };
        let server = common::create_server(&config).expect("server builds in test");
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
            std::env::remove_var("HTTP_AUTH_TOKEN");
            std::env::remove_var("HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED");
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
    async fn malformed_origin_non_utf8_returns_403() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let mut request = mcp_post("/mcp/").body(Body::from("{}")).expect("build request");
        request.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_bytes(&[0xFF, 0xFE]).expect("non-utf8 header value"),
        );
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
        assert_eq!(body, "Forbidden: Invalid Origin header encoding");
    }

    #[tokio::test]
    async fn malformed_origin_unparseable_returns_403() {
        let router = router_with_origins(HttpConfig::default_allowed_origins());
        let request = mcp_post("/mcp/")
            .header(header::ORIGIN, "not a url")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, body) = send(router, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
        assert_eq!(body, "Forbidden: Invalid Origin header");
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

    #[test]
    fn try_from_rejects_non_loopback_without_token() {
        let args = parse_http_args(&["--host", "0.0.0.0"]);
        let errors = HttpConfig::try_from(&args).expect_err("0.0.0.0 without token must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::UnauthenticatedNonLoopbackBind(_)))
        );
    }

    #[test]
    fn try_from_accepts_non_loopback_with_token() {
        let args = parse_http_args(&["--host", "0.0.0.0", "--auth-token", TEST_TOKEN]);
        let config = HttpConfig::try_from(&args).expect("token must allow non-loopback bind");
        assert_eq!(config.auth_token.as_deref(), Some(TEST_TOKEN));
    }

    #[test]
    fn try_from_accepts_non_loopback_with_opt_out() {
        let args = parse_http_args(&["--host", "0.0.0.0", "--dangerously-allow-unauthenticated"]);
        let config = HttpConfig::try_from(&args).expect("opt-out must allow non-loopback bind");
        assert!(config.allow_unauthenticated);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn http_arguments_debug_redacts_token() {
        let args = parse_http_args(&["--auth-token", TEST_TOKEN]);
        let rendered = format!("{args:?}");
        assert!(!rendered.contains(TEST_TOKEN), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
    }

    async fn assert_unauthorized(request: Request<Body>) {
        let response = router_with_token().oneshot(request).await.expect("oneshot");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE),
            Some(&HeaderValue::from_static(r#"Bearer realm="dbmcp""#))
        );
    }

    #[tokio::test]
    async fn token_missing_header_returns_401() {
        assert_unauthorized(mcp_post("/mcp/").body(Body::from("{}")).expect("build request")).await;
    }

    #[tokio::test]
    async fn token_wrong_value_returns_401() {
        let request = mcp_post("/mcp/")
            .header(header::AUTHORIZATION, "Bearer garbage")
            .body(Body::from("{}"))
            .expect("build request");
        assert_unauthorized(request).await;
    }

    #[tokio::test]
    async fn token_wrong_scheme_returns_401() {
        let request = mcp_post("/mcp/")
            .header(header::AUTHORIZATION, format!("Basic {TEST_TOKEN}"))
            .body(Body::from("{}"))
            .expect("build request");
        assert_unauthorized(request).await;
    }

    #[tokio::test]
    async fn token_prefix_of_expected_returns_401() {
        let request = mcp_post("/mcp/")
            .header(header::AUTHORIZATION, "Bearer test-token")
            .body(Body::from("{}"))
            .expect("build request");
        assert_unauthorized(request).await;
    }

    #[tokio::test]
    async fn token_valid_passes_auth() {
        for scheme in ["Bearer", "bearer"] {
            let request = mcp_post("/mcp/")
                .header(header::AUTHORIZATION, format!("{scheme} {TEST_TOKEN}"))
                .body(Body::from("{}"))
                .expect("build request");
            let (status, body) = send(router_with_token(), request).await;
            assert_ne!(status, StatusCode::UNAUTHORIZED, "scheme={scheme} body={body}");
            assert_ne!(status, StatusCode::FORBIDDEN, "scheme={scheme} body={body}");
        }
    }

    #[tokio::test]
    async fn token_valid_still_enforces_host_allowlist() {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/mcp/")
            .header(header::HOST, "evil.example")
            .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from("{}"))
            .expect("build request");
        let (status, body) = send(router_with_token(), request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
        assert_eq!(body, "Forbidden: Host header is not allowed");
    }

    #[tokio::test]
    async fn token_cors_preflight_skips_auth() {
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/mcp/")
            .header(header::HOST, "localhost")
            .header(header::ORIGIN, "http://localhost")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization, content-type")
            .body(Body::empty())
            .expect("build request");
        let response = router_with_token().oneshot(request).await.expect("oneshot");
        assert!(response.status().is_success(), "status={}", response.status());
    }
}
