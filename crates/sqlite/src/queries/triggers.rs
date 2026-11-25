//! Trigger listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::SqliteConnection;

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `SQLite`'s `LIKE` operator is case-insensitive for ASCII by default and
/// `dbmcp` does not toggle the `case_sensitive_like` PRAGMA, so a bare
/// `LIKE` already matches case-insensitively. (A `COLLATE NOCASE` clause on
/// the right-hand pattern would be a no-op — `LIKE` does not honor RHS
/// collation; see <https://www.sqlite.org/lang_expr.html>.) User-facing
/// `LIKE` wildcards (`%`, `_`) in `?1` flow straight through.
const NAMES: &str = r"
    SELECT name
    FROM sqlite_schema
    WHERE type = 'trigger'
      AND name NOT LIKE 'sqlite_%'
      AND (?1 IS NULL OR name LIKE '%' || ?1 || '%')
    ORDER BY name
    LIMIT ?2 OFFSET ?3";

/// Lists one `page` of trigger names, filtered by `pattern` when given.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(
    connection: &SqliteConnection,
    page: Page,
    pattern: Option<&str>,
) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(
            sqlx::query(NAMES).bind(pattern).bind(page.limit()).bind(page.offset()),
            None,
        )
        .await
}

/// Detailed-mode SQL: single SELECT projecting `(name, json_object(...))`.
///
/// `'main'` is hard-coded in the projection — `sqlite_schema` here is the
/// `main` schema's catalogue, the only one the connection helper opens.
/// `AND sql IS NOT NULL` enforces the homogeneous-shape contract: rows whose
/// stored `CREATE TRIGGER` text is `NULL` (extension-generated, hand-edited)
/// are silently omitted from detailed pages. Brief mode does not filter on
/// `sql` so those triggers are still discoverable by name. `SQLite` trigger
/// names are unique within a database, so `ORDER BY name` is sufficient
/// for stable pagination — no secondary tie-breaker needed.
const DETAILS: &str = r"
    SELECT
        name,
        json_object(
            'schema',     'main',
            'table',      tbl_name,
            'definition', sql
        ) AS entry
    FROM sqlite_schema
    WHERE type = 'trigger'
      AND name NOT LIKE 'sqlite_%'
      AND sql IS NOT NULL
      AND (?1 IS NULL OR name LIKE '%' || ?1 || '%')
    ORDER BY name
    LIMIT ?2 OFFSET ?3";

/// Lists one `page` of trigger names with their detail objects.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn details(
    connection: &SqliteConnection,
    page: Page,
    pattern: Option<&str>,
) -> Result<Vec<(String, Value)>, SqlError> {
    connection
        .fetch(
            sqlx::query(DETAILS)
                .bind(pattern)
                .bind(page.limit())
                .bind(page.offset()),
            None,
        )
        .await
}
