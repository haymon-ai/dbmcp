//! Server construction and process lifecycle helpers.
//!
//! [`create_server`] maps the configured [`DatabaseBackend`] onto the
//! matching concrete adapter, tunnelling through SSH when configured.
//! [`shutdown_signal`] resolves on Ctrl-C or `SIGTERM`.

use dbmcp_config::{Config, DatabaseBackend};
use dbmcp_mysql::MysqlHandler;
use dbmcp_postgres::PostgresHandler;
use dbmcp_server::{ErrorContext, Server};
use dbmcp_sql::is_connection_error;
use dbmcp_sqlite::SqliteHandler;
use dbmcp_ssh::{Tunnel, TunnelHandle};
use tracing::info;

use crate::error::ErrorKind;

/// Logs the read-only banner and builds a [`Server`] for `config`.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable. The caller is expected to pass a fully
/// validated [`Config`].
///
/// When `config.ssh` is set, starts the SSH tunnel first and points the
/// adapter at its loopback address. The returned [`TunnelHandle`] must
/// outlive the [`Server`] so pool shutdown traffic reaches the database;
/// binding the tuple in order (`let (tunnel, server)`) drops the server
/// first on every return path. While the tunnel reports a failure, tool
/// errors caused by an unreachable database gain a `(SSH tunnel: <cause>)`
/// suffix before PII redaction; other errors are left alone.
///
/// # Errors
///
/// Returns [`ErrorKind::Init`] when the SSH tunnel cannot start
/// or a handler fails to initialise (e.g. a fail-closed NER model load).
#[allow(clippy::result_large_err)]
pub(crate) async fn create_server(config: &Config) -> Result<(Option<TunnelHandle>, Server), ErrorKind> {
    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    let Some(ssh) = &config.ssh else {
        return Ok((None, build_server(config)?));
    };

    let tunnel = Tunnel::start(ssh, &config.database.host, config.database.port)
        .await
        .map_err(|e| ErrorKind::Init(e.to_string()))?;
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
    let server = build_server(&tunnelled)?.with_error_context(context);
    Ok((Some(tunnel), server))
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
