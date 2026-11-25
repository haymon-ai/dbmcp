//! View listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};

use crate::connection::SqliteConnection;

/// Lists one page of user view names, sorted by name.
const NAMES: &str = r"
    SELECT name
    FROM sqlite_schema
    WHERE type = 'view' AND name NOT LIKE 'sqlite_%'
    ORDER BY name
    LIMIT ?1 OFFSET ?2";

/// Lists one `page` of view names.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(connection: &SqliteConnection, page: Page) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(sqlx::query(NAMES).bind(page.limit()).bind(page.offset()), None)
        .await
}
