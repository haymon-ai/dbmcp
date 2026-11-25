//! Database listing statements and `CREATE` / `DROP DATABASE`.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};

use super::ident;
use crate::connection::MysqlConnection;

/// Lists one page of schema names, sorted by name.
const NAMES: &str = r"
    SELECT CAST(SCHEMA_NAME AS CHAR)
    FROM information_schema.SCHEMATA
    ORDER BY SCHEMA_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of database names through the default pool.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(connection: &MysqlConnection, page: Page) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(sqlx::query(NAMES).bind(page.limit()).bind(page.offset()), None)
        .await
}

/// Reads the name of one schema, bound exactly.
const EXISTS: &str = "SELECT CAST(SCHEMA_NAME AS CHAR) \
                     FROM information_schema.SCHEMATA \
                     WHERE SCHEMA_NAME = ?";

/// Tells whether `database` exists, compared exactly.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn exists(connection: &MysqlConnection, database: &str) -> Result<bool, SqlError> {
    let found: Option<String> = connection
        .fetch_optional(sqlx::query(EXISTS).bind(database), None)
        .await?;
    Ok(found.is_some())
}

/// Statement prefix of `create`; the quoted database name follows.
const CREATE: &str = "CREATE DATABASE IF NOT EXISTS";

/// Creates `database` unless it exists, quoted as an identifier.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed statement, or
/// [`SqlError::ReadOnlyViolation`] in read-only mode.
pub(crate) async fn create(connection: &MysqlConnection, database: &str) -> Result<(), SqlError> {
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
pub(crate) async fn drop(connection: &MysqlConnection, database: &str) -> Result<(), SqlError> {
    connection
        .execute(format!("{DROP} {}", ident::quote(database)).as_str(), None)
        .await?;
    Ok(())
}
