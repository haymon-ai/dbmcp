//! Connection hook statements: `query_only` and the library version.

/// Forbids writes on the connection until switched off again.
const QUERY_ONLY_ON: &str = "PRAGMA query_only = ON";

/// Allows writes on the connection again.
const QUERY_ONLY_OFF: &str = "PRAGMA query_only = OFF";

/// Switches `PRAGMA query_only` on or off for `conn`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed pragma.
pub(crate) async fn query_only(conn: &mut sqlx::SqliteConnection, on: bool) -> Result<(), sqlx::Error> {
    let pragma = if on { QUERY_ONLY_ON } else { QUERY_ONLY_OFF };
    sqlx::query(pragma).execute(conn).await?;
    Ok(())
}

/// Reads the `SQLite` library version.
const VERSION: &str = "SELECT sqlite_version()";

/// Reads the `SQLite` library version string, such as `3.50.4`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn version(conn: &mut sqlx::SqliteConnection) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(VERSION).fetch_one(conn).await
}
