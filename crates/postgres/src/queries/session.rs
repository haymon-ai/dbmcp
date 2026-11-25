//! Connection hook statements: backend pid, timeout, cancel, and version probes.

use std::time::Duration;

/// Reads the server process id of the session.
const ID: &str = "SELECT pg_backend_pid()";

/// Reads the server process id serving `conn`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn id(conn: &mut sqlx::PgConnection) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(ID).fetch_one(conn).await
}

/// Sets the session `statement_timeout` to `$1` milliseconds.
const SET_TIMEOUT: &str = "SELECT set_config('statement_timeout', $1, false)";

/// Sets the session `statement_timeout` of `conn` to `limit`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn set_timeout(conn: &mut sqlx::PgConnection, limit: Duration) -> Result<(), sqlx::Error> {
    let millis = limit.as_millis().to_string();
    sqlx::query(SET_TIMEOUT).bind(millis).execute(conn).await.map(|_| ())
}

/// Cancels the running statement of backend `$1`.
const KILL: &str = "SELECT pg_cancel_backend($1)";

/// Cancels the running statement of backend `pid`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn kill(conn: &mut sqlx::PgConnection, pid: i32) -> Result<(), sqlx::Error> {
    sqlx::query(KILL).bind(pid).execute(conn).await.map(|_| ())
}

/// Reads the full server version text.
const VERSION: &str = "SELECT pg_catalog.version()";

/// Reads the server version text, such as `PostgreSQL 17.5 on x86_64-pc-linux-gnu, …`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn version(conn: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(VERSION).fetch_one(conn).await
}

/// Probes for Aurora's `aurora_version()` function without calling it.
const AURORA_VERSION: &str = "SELECT pg_catalog.to_regproc('pg_catalog.aurora_version') IS NOT NULL";

/// Tells whether the server defines `pg_catalog.aurora_version`, as Aurora `PostgreSQL` does.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn aurora_version(conn: &mut sqlx::PgConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(AURORA_VERSION).fetch_one(conn).await
}
