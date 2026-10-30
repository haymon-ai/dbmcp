//! Server construction and process lifecycle helpers.
//!
//! [`serve`] maps the configured [`DatabaseBackend`] onto the matching
//! concrete adapter, tunnelling through SSH when configured, and runs it
//! over the selected transport until shutdown.
//! [`shutdown_signal`] resolves on Ctrl-C or `SIGTERM`.

use dbmcp_config::{Config, DatabaseBackend, Transport};
use dbmcp_mysql::MysqlHandler;
use dbmcp_postgres::PostgresHandler;
use dbmcp_server::{ErrorContext, Server};
use dbmcp_sql::is_connection_error;
use dbmcp_sqlite::SqliteHandler;
use dbmcp_ssh::Tunnel;
use tracing::info;

use crate::error::ErrorKind;
use crate::{http, stdio};

/// Builds the [`Server`] for `config`, serves its transport, then closes the pools.
///
/// Does **not** establish a database connection up front. Each adapter
/// defers pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable. The caller is expected to pass a fully
/// validated [`Config`].
///
/// When `config.ssh` is set, starts the SSH tunnel first and points the
/// adapter at its loopback address. While the tunnel reports a failure,
/// tool errors caused by an unreachable database gain a
/// `(SSH tunnel: <cause>)` suffix before PII redaction; other errors are
/// left alone. The pools are shut down on every exit path once the
/// transport stops, including when it fails.
///
/// # Errors
///
/// Returns [`ErrorKind::Init`] when the SSH tunnel cannot start or a
/// handler fails to initialise (e.g. a fail-closed NER model load), and
/// the transport's error when it fails to start or serve.
///
/// # Panics
///
/// Never in practice: the HTTP config is always built alongside the
/// `http` transport.
#[allow(clippy::result_large_err)]
pub(crate) async fn serve(config: &Config) -> Result<(), ErrorKind> {
    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    // Pools close through the tunnel, so it is declared before the server
    // and therefore dropped after it on every return path.
    let tunnel = match &config.ssh {
        Some(ssh) => Some(
            Tunnel::start(ssh, &config.database.host, config.database.port)
                .await
                .map_err(|e| ErrorKind::Init(e.to_string()))?,
        ),
        None => None,
    };
    let server = match &tunnel {
        None => build_server(config)?,
        Some(tunnel) => {
            let local_addr = tunnel.local_addr();
            let mut tunnelled = config.clone();
            tunnelled.database.host = local_addr.ip().to_string();
            tunnelled.database.port = local_addr.port();
            let last_error = tunnel.last_error().clone();
            let context = ErrorContext::new(move |error| {
                is_connection_error(error)
                    .then(|| last_error.get())
                    .flatten()
                    .map(|cause| format!("SSH tunnel: {cause}"))
            });
            build_server(&tunnelled)?.with_error_context(context)
        }
    };

    let result = match config.transport {
        Transport::Stdio => stdio::serve(server.clone()).await,
        Transport::Http => {
            let http_config = config
                .http
                .as_ref()
                .expect("http config is built when transport is http");
            http::serve(server.clone(), http_config).await
        }
    };

    server.shutdown().await;
    info!("All connection pools closed.");
    result
}

/// Builds the backend adapter selected by `config.database.backend`.
#[allow(clippy::result_large_err)]
pub(crate) fn build_server(config: &Config) -> Result<Server, ErrorKind> {
    match config.database.backend {
        DatabaseBackend::Sqlite => init(SqliteHandler::new(config)),
        DatabaseBackend::Postgres => init(PostgresHandler::new(config)),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => init(MysqlHandler::new(config)),
    }
}

/// Maps a handler constructor result into a [`Server`], failing closed.
#[allow(clippy::result_large_err)]
fn init<H, E>(handler: Result<H, E>) -> Result<Server, ErrorKind>
where
    H: Into<Server>,
    E: std::fmt::Display,
{
    handler.map(Into::into).map_err(|e| ErrorKind::Init(e.to_string()))
}

/// Future that resolves when the process should shut down.
///
/// Listens for Ctrl-C on all platforms and `SIGTERM` on Unix, which
/// is the signal `docker stop`, `systemctl stop`, and Kubernetes
/// send to request graceful termination. Whichever arrives first
/// wins. Shared by both transports.
pub(crate) async fn shutdown_signal() {
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
