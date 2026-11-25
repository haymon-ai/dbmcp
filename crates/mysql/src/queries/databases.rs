//! Database listing statement.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};

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
