//! HTTP transport command.
//!
//! Runs the MCP server over Streamable HTTP with CORS support and
//! optional bearer-token authentication. Each HTTP session clones the
//! internally-built handler, sharing the underlying connection pools.

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use clap::builder::BoolishValueParser;
use clap::{ArgAction, ArgMatches, Args, FromArgMatches, Parser};
use dbmcp_config::{AllowedOrigins, Config, ConfigLayer, ExposeSecret, HttpConfig, HttpLayer, SecretString};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{info, warn};

use crate::commands::common::{self, DatabaseArguments, PiiArguments, SharedArguments, SshArguments, explicit};
use crate::error::ErrorKind;

/// HTTP transport flags embedded in [`HttpCommand`].
///
/// Each clap id is the setting's config path (`http.host`), so `host` and
/// `port` don't collide with [`DatabaseArguments`] when both are flattened
/// into [`HttpCommand`].
#[derive(Debug, Args)]
#[command(next_help_heading = "HTTP Transport")]
struct HttpArguments {
    /// Bind host for HTTP transport.
    #[arg(
        id = "http.host",
        long = "host",
        env = "HTTP_HOST",
        value_name = "HOST",
        default_value = HttpConfig::DEFAULT_HOST,
    )]
    host: String,

    /// Bind port for HTTP transport.
    #[arg(
        id = "http.port",
        long = "port",
        env = "HTTP_PORT",
        value_name = "PORT",
        default_value_t = HttpConfig::DEFAULT_PORT
    )]
    port: u16,

    /// Allowed browser origins (comma-separated, RFC 6454 `<scheme>://<host>[:<port>]` or `null`).
    ///
    /// Drives BOTH the CORS preflight allowlist AND the rmcp server-side
    /// Origin validator. Use `:*` to match any port or give an explicit
    /// port; entries without a port are deprecated. Malformed entries are
    /// rejected at startup. Pass an empty list
    /// (`--allowed-origins ""`) to disable server-side Origin validation
    /// and grant no CORS access — only do this for non-browser deployments.
    #[arg(
        id = "http.allowed_origins", long = "allowed-origins",
        env = "HTTP_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_origins()
    )]
    allowed_origins: Vec<String>,

    /// Allowed host names (comma-separated).
    #[arg(
        id = "http.allowed_hosts", long = "allowed-hosts",
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
        id = "http.auth_token",
        long = "auth-token",
        env = "HTTP_AUTH_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    auth_token: Option<SecretString>,

    /// Allow binding a non-loopback host without `HTTP_AUTH_TOKEN`.
    ///
    /// Only for deployments where a reverse proxy authenticates callers.
    /// Anyone who can reach the port gets full access to the database.
    #[arg(
        id = "http.allow_unauthenticated",
        long = "dangerously-allow-unauthenticated",
        env = "HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED",
        default_value_t = false,
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    dangerously_allow_unauthenticated: bool,
}

impl HttpArguments {
    /// Returns the explicitly set HTTP flags as a layer.
    fn layer(self, matches: &ArgMatches) -> HttpLayer {
        HttpLayer {
            host: explicit(matches, "http.host", Some(self.host)),
            port: explicit(matches, "http.port", Some(self.port)),
            // `--allowed-origins ""` arrives as `[""]`; dropping blanks turns
            // it into the empty list that disables Origin validation.
            allowed_origins: explicit(matches, "http.allowed_origins", Some(self.allowed_origins))
                .map(|origins| origins.into_iter().filter(|origin| !origin.trim().is_empty()).collect()),
            allowed_hosts: explicit(matches, "http.allowed_hosts", Some(self.allowed_hosts)),
            auth_token: explicit(matches, "http.auth_token", self.auth_token),
            allow_unauthenticated: explicit(
                matches,
                "http.allow_unauthenticated",
                Some(self.dangerously_allow_unauthenticated),
            ),
        }
    }
}

/// Runs the MCP server in HTTP mode.
#[derive(Debug, Parser)]
pub(crate) struct HttpCommand {
    /// Config-file and logging flags.
    #[command(flatten)]
    shared: SharedArguments,

    /// Shared database connection flags.
    #[command(flatten)]
    database: DatabaseArguments,

    /// HTTP transport flags.
    #[command(flatten)]
    http: HttpArguments,

    /// Shared PII flags.
    #[command(flatten)]
    pii: PiiArguments,

    /// Shared SSH tunnel flags.
    #[command(flatten)]
    ssh: SshArguments,
}

impl HttpCommand {
    /// Returns the flags in `matches` set by flag or env var as a layer.
    ///
    /// # Panics
    ///
    /// Panics if `matches` were not parsed by [`Self::command`].
    pub(crate) fn layer(matches: &ArgMatches) -> ConfigLayer {
        let cmd = Self::from_arg_matches(matches).expect("matches were parsed by HttpCommand::command()");
        ConfigLayer {
            log_level: cmd.shared.log_level(matches),
            database: cmd.database.layer(matches),
            http: cmd.http.layer(matches),
            pii: cmd.pii.layer(matches),
            ssh: cmd.ssh.layer(matches),
        }
    }

    /// Builds the server for `config` and runs the HTTP transport.
    ///
    /// Binds to the configured host/port and serves MCP requests over
    /// Streamable HTTP. Each session clones the internally-built handler,
    /// sharing the underlying database connection pools. Shuts down
    /// gracefully on Ctrl-C or `SIGTERM`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be built, TCP bind fails
    /// (port in use, permission denied), or the HTTP service fails to
    /// serve.
    ///
    /// # Panics
    ///
    /// Panics if `config.http` is `None`; [`Self::load`] always sets it.
    pub(crate) async fn execute(config: Config) -> Result<(), ErrorKind> {
        let http_config = config.http.as_ref().expect("http config set by Transport::Http");

        let (tunnel, server) = common::create_server(&config).await?;
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

        // Pools close through the tunnel, so it must outlive the server.
        drop(server);
        drop(tunnel);
        Ok(())
    }
}

/// Builds the axum router that serves MCP over Streamable HTTP.
///
/// Wires the configured allowed-origins list into BOTH the rmcp
/// server-side Origin validator and the tower-http CORS preflight layer,
/// which match entries with the same rules via [`AllowedOrigins`]. An
/// empty list disables rmcp's validator but grants no CORS access, so
/// browsers stay blocked. When an auth token is configured, every request
/// must pass [`require_bearer`] before it reaches the MCP service. CORS stays outermost so browser preflights,
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
            BearerAuth::new(token.expose_secret()),
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
///
/// Uses the same `(scheme, host, port)` matching as rmcp, including the
/// `:*` port wildcard, instead of tower-http's exact string matching.
fn build_cors_layer(http_config: &HttpConfig) -> CorsLayer {
    let origins = AllowedOrigins::new(&http_config.allowed_origins);

    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| origins.allows(origin)))
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
    use clap::{CommandFactory, Parser};
    use dbmcp_config::{
        ConfigError, ConfigErrors, DatabaseBackend, DatabaseConfig, LoadError, PiiConfig, Resolved, Transport,
    };

    use crate::commands::common::tests_support;
    use tower::ServiceExt;

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct TestCli {
        #[command(flatten)]
        http: HttpArguments,
    }

    /// Parses `args` and resolves them into an [`HttpConfig`].
    fn http_config(args: &[&str]) -> Result<HttpConfig, ConfigErrors> {
        tests_support::clear_env::<HttpCommand>(&[]);
        let matches = TestCli::command()
            .try_get_matches_from(args)
            .unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
        let cli = TestCli::from_arg_matches(&matches).expect("from matches");
        let layer = ConfigLayer {
            http: cli.http.layer(&matches),
            ..ConfigLayer::default()
        };
        let resolved = layer.resolve(Transport::Http, None)?;
        Ok(resolved.config.http.expect("http transport builds http"))
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
            auth_token: auth_token.map(SecretString::from),
            allow_unauthenticated: false,
        };
        let config = Config {
            database: sqlite_memory_db_config(),
            http: Some(http_config.clone()),
            pii: PiiConfig::default(),
            ssh: None,
        };
        let server = common::build_server(&config).expect("server builds in test");
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
    fn default_http_args_validate() {
        http_config(&[]).expect("default http args must validate");
    }

    /// Parses `args` as the full `http` subcommand and loads its config.
    fn load_http(args: &[&str]) -> Result<Resolved, LoadError> {
        tests_support::load::<HttpCommand>(HttpCommand::layer, Transport::Http, args)
    }

    #[test]
    fn load_accumulates_database_and_http_errors_in_order() {
        let error = load_http(&["_", "--db-backend", "sqlite", "--host", ""]).expect_err("must fail");
        let LoadError::Invalid { errors, .. } = error else {
            panic!("expected validation error");
        };
        assert_eq!(errors.len(), 2);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(errors[1], ConfigError::EmptyHttpHost));
    }

    #[test]
    fn load_reads_http_section_from_config_file() {
        let (_dir, path) = tests_support::config_file("http:\n  port: 9100\n  allowed_hosts: [db.example]\n");
        let path = path.to_str().expect("utf-8 path");
        let resolved = load_http(&["_", "--config", path, "--allowed-hosts", "cli.example"]).expect("valid");
        let http = resolved.config.http.expect("http config");
        assert_eq!(http.port, 9100, "file fills unset flag");
        assert_eq!(http.allowed_hosts, ["cli.example"], "flag replaces file list");
        assert_eq!(http.host, HttpConfig::DEFAULT_HOST, "default fills the rest");
    }

    #[test]
    fn every_setting_flag_has_a_route() {
        tests_support::check_routes::<HttpCommand>(HttpCommand::layer);
    }

    #[test]
    fn clap_default_yields_loopback_origins() {
        let config = http_config(&[]).expect("default args are valid");
        assert_eq!(config.allowed_origins, HttpConfig::default_allowed_origins());
    }

    #[test]
    fn empty_and_whitespace_host_are_rejected() {
        for host in ["", "   "] {
            let errors = http_config(&["--host", host]).expect_err("blank host must be rejected by validate");
            assert!(
                errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)),
                "{host:?}"
            );
        }
    }

    #[test]
    fn clap_accepts_default_host() {
        assert_eq!(http_config(&[]).expect("valid").host, HttpConfig::DEFAULT_HOST);
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

    fn preflight(origin: &str) -> Request<Body> {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/mcp/")
            .header(header::HOST, "localhost")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization, content-type")
            .body(Body::empty())
            .expect("build request")
    }

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

    /// Asserts the CORS layer and rmcp's validator both accept or both reject `origin`.
    async fn assert_origin(origins: &[impl AsRef<str>], origin: &str, allowed: bool) {
        let origins: Vec<String> = origins.iter().map(|o| o.as_ref().to_owned()).collect();
        let context = format!("origins={origins:?} origin={origin}");
        let router = router_with_origins(origins);

        let response = router.clone().oneshot(preflight(origin)).await.expect("oneshot");
        let allow_origin = response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN);
        let post = mcp_post("/mcp/")
            .header(header::ORIGIN, origin)
            .body(Body::from(INITIALIZE))
            .expect("build request");
        let (status, body) = send(router, post).await;

        if allowed {
            assert_eq!(
                allow_origin.and_then(|v| v.to_str().ok()),
                Some(origin),
                "preflight {context}"
            );
            assert_eq!(status, StatusCode::OK, "POST {context} body={body}");
        } else {
            assert_eq!(allow_origin, None, "preflight {context}");
            assert_eq!(status, StatusCode::FORBIDDEN, "POST {context} body={body}");
            assert_eq!(body, "Forbidden: Origin header is not allowed", "POST {context}");
        }
    }

    /// Pins `crate::commands::origin` to rmcp's private matcher.
    ///
    /// Every case runs through both the CORS preflight and rmcp's
    /// validator, so an rmcp upgrade that changes Origin semantics fails
    /// here instead of silently splitting the two layers.
    #[tokio::test]
    async fn cors_and_rmcp_agree_on_origin_matching() {
        let cases: &[(&[&str], &str, bool)] = &[
            (&["http://app.example:*"], "http://app.example", true),
            (&["http://app.example:*"], "http://app.example:5173", true),
            (&["http://app.example:8080"], "http://app.example:8080", true),
            (&["http://app.example:8080"], "http://app.example:9090", false),
            (&["http://app.example:8080"], "http://app.example", false),
            (&["https://app.example:443"], "https://app.example", true),
            (&["https://app.example:443"], "http://app.example", false),
            (&["http://app.example"], "http://app.example", true),
            (&["http://app.example"], "http://app.example:5173", true),
            (&["HTTP://App.Example:*"], "http://APP.example:3000", true),
            (&["http://[::1]:*"], "http://[::1]:8080", true),
            (&["http://[::1]:*"], "http://[::2]:8080", false),
            (&["null"], "null", true),
            (&["http://app.example:*"], "null", false),
            (&["http://app.example:*"], "https://app.example", false),
            (&["http://app.example:*"], "http://evil.example", false),
            (&["not a url", "http://app.example:*"], "http://app.example", true),
        ];
        for (origins, origin, allowed) in cases {
            assert_origin(origins, origin, *allowed).await;
        }
    }

    #[tokio::test]
    async fn default_origins_allow_any_local_port() {
        for (origin, allowed) in [
            ("http://localhost", true),
            ("http://localhost:5173", true),
            ("http://127.0.0.1:3000", true),
            ("https://localhost:8443", true),
            ("https://127.0.0.1", true),
            ("http://[::1]:5173", true),
            ("http://evil.example:5173", false),
        ] {
            assert_origin(&HttpConfig::default_allowed_origins(), origin, allowed).await;
        }
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
    fn empty_allowed_origins_flag_yields_empty_list() {
        let config = http_config(&["--allowed-origins", ""]).expect("empty origins must validate");
        assert!(config.allowed_origins.is_empty(), "{:?}", config.allowed_origins);
    }

    #[test]
    fn rejects_malformed_origin() {
        let errors = http_config(&["--allowed-origins", "http://localhost:*,localhost"]).expect_err("malformed origin");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::InvalidAllowedOrigin(entry) if entry == "localhost"))
        );
    }

    #[test]
    fn rejects_non_loopback_without_token() {
        let errors = http_config(&["--host", "0.0.0.0"]).expect_err("0.0.0.0 without token must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::UnauthenticatedNonLoopbackBind(_)))
        );
    }

    #[test]
    fn accepts_non_loopback_with_token() {
        let config = http_config(&["--host", "0.0.0.0", "--auth-token", TEST_TOKEN]).expect("token allows bind");
        assert_eq!(
            config.auth_token.as_ref().map(ExposeSecret::expose_secret),
            Some(TEST_TOKEN)
        );
    }

    #[test]
    fn accepts_non_loopback_with_opt_out() {
        let config =
            http_config(&["--host", "0.0.0.0", "--dangerously-allow-unauthenticated"]).expect("opt-out allows bind");
        assert!(config.allow_unauthenticated);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn dangerously_allow_unauthenticated_accepts_explicit_values() {
        let allowed = |args: &[&str]| http_config(args).expect("valid").allow_unauthenticated;
        assert!(!allowed(&[]));
        assert!(allowed(&["--dangerously-allow-unauthenticated=true"]));
        for value in ["false", "0", "no", "off"] {
            let arg = format!("--dangerously-allow-unauthenticated={value}");
            assert!(!allowed(&[&arg]), "{value:?}");
        }
        for value in ["", "disabled", "never"] {
            let arg = format!("--dangerously-allow-unauthenticated={value}");
            assert!(TestCli::try_parse_from([&arg]).is_err(), "{value:?} must be rejected");
        }
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
        let response = router_with_token()
            .oneshot(preflight("http://localhost"))
            .await
            .expect("oneshot");
        assert!(response.status().is_success(), "status={}", response.status());
    }
}
