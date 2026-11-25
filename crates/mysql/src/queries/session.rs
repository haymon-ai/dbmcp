//! Connection hook statements: read-only mode, version probes, session id, timeout, and kill.

use std::time::Duration;

use dbmcp_types::database;
use sqlx::AssertSqlSafe;
use sqlx::mysql::MySqlDatabaseError;

/// Makes every later transaction of the session read-only.
const READ_ONLY: &str = "SET SESSION TRANSACTION READ ONLY";

/// Makes the session of `conn` read-only, so the server rejects writes.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed statement.
pub(crate) async fn read_only(conn: &mut sqlx::MySqlConnection) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(READ_ONLY).execute(conn).await.map(|_| ())
}

/// Reads the server version text.
const VERSION: &str = "SELECT CAST(VERSION() AS CHAR)";

/// Reads the server version text, such as `8.4.5` or `11.4.7-MariaDB`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn version(conn: &mut sqlx::MySqlConnection) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(VERSION).fetch_one(conn).await
}

/// Lists Aurora's `aurora_version` variable, which only Aurora `MySQL` defines.
const AURORA_VERSION: &str = "SHOW VARIABLES LIKE 'aurora_version'";

/// Tells whether the server defines `aurora_version`, as Aurora `MySQL` does.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn aurora_version(conn: &mut sqlx::MySqlConnection) -> Result<bool, sqlx::Error> {
    Ok(sqlx::raw_sql(AURORA_VERSION).fetch_optional(conn).await?.is_some())
}

/// Reads the connection id of the session.
const ID: &str = "SELECT CONNECTION_ID()";

/// Reads the connection id serving `conn`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn id(conn: &mut sqlx::MySqlConnection) -> Result<u64, sqlx::Error> {
    sqlx::query_scalar(ID).fetch_one(conn).await
}

/// Sets the `MariaDB` session statement timeout, in seconds.
const SET_TIMEOUT_MARIADB: &str = "SET SESSION max_statement_time = ?";

/// Sets the `MySQL` session statement timeout, in milliseconds.
const SET_TIMEOUT: &str = "SET SESSION max_execution_time = ?";

/// Sets the session statement timeout of `conn` to `limit`, in `backend`'s variable.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed statement.
pub(crate) async fn set_timeout(
    conn: &mut sqlx::MySqlConnection,
    backend: database::Backend,
    limit: Duration,
) -> Result<(), sqlx::Error> {
    let query = match backend {
        database::Backend::Mariadb => sqlx::query(SET_TIMEOUT_MARIADB).bind(limit.as_secs_f64()),
        _ => sqlx::query(SET_TIMEOUT).bind(u64::try_from(limit.as_millis()).unwrap_or(u64::MAX)),
    };
    query.execute(conn).await.map(|_| ())
}

/// Statement prefix of `kill`; the connection id follows.
const KILL: &str = "KILL QUERY";

/// Stops the running statement of connection `id`; an ended session is no error.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed statement other than unknown thread id (1094).
pub(crate) async fn kill(conn: &mut sqlx::MySqlConnection, id: u64) -> Result<(), sqlx::Error> {
    // `id` is a server-returned integer, not user input. MySQL 9 prepares
    // `KILL QUERY ?`, but sqlx can't decode its parameter type (0xf3).
    match sqlx::raw_sql(AssertSqlSafe(format!("{KILL} {id}"))).execute(conn).await {
        // 1094: unknown thread id, the session already ended.
        Err(error)
            if error
                .as_database_error()
                .and_then(|e| e.try_downcast_ref::<MySqlDatabaseError>())
                .is_some_and(|e| e.number() == 1094) =>
        {
            Ok(())
        }
        result => result.map(|_| ()),
    }
}
