//! Database listing statements and `CREATE` / `DROP DATABASE`.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};

use super::ident;
use crate::connection::PostgresConnection;

/// Lists one page of non-template database names, sorted by name.
const NAMES: &str = r"
    SELECT datname
    FROM pg_database
    WHERE datistemplate = false
    ORDER BY datname
    LIMIT $1 OFFSET $2";

/// Lists one `page` of database names through the default pool.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(connection: &PostgresConnection, page: Page) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(sqlx::query(NAMES).bind(page.limit()).bind(page.offset()), None)
        .await
}

/// Statement prefix of `create`; the quoted database name follows.
const CREATE: &str = "CREATE DATABASE";

/// Creates `database`, quoted as an identifier.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed statement, or
/// [`SqlError::ReadOnlyViolation`] in read-only mode.
pub(crate) async fn create(connection: &PostgresConnection, database: &str) -> Result<(), SqlError> {
    connection
        .execute(format!("{CREATE} {}", ident::quote(database)).as_str(), None)
        .await?;
    Ok(())
}

/// Statement prefix of `drop`; the quoted database name follows.
const DROP: &str = "DROP DATABASE";

/// Drops `database`, quoted as an identifier.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed statement, or
/// [`SqlError::ReadOnlyViolation`] in read-only mode.
pub(crate) async fn drop(connection: &PostgresConnection, database: &str) -> Result<(), SqlError> {
    connection
        .execute(format!("{DROP} {}", ident::quote(database)).as_str(), None)
        .await?;
    Ok(())
}
