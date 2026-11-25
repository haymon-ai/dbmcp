//! Database listing statement.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};

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
