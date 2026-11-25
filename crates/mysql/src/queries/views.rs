//! View listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::MysqlConnection;

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(TABLE_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
/// `(? IS NULL OR ...)` lets one prepared statement cover both filtered and
/// unfiltered cases.
const NAMES: &str = r"
    SELECT CAST(TABLE_NAME AS CHAR)
    FROM information_schema.VIEWS
    WHERE TABLE_SCHEMA = ?
      AND (? IS NULL OR LOWER(TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TABLE_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of view names in `database`, filtered by `pattern` when given.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(
    connection: &MysqlConnection,
    page: Page,
    pattern: Option<&str>,
    database: &str,
) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(
            sqlx::query(NAMES)
                .bind(database)
                .bind(pattern)
                .bind(pattern)
                .bind(page.limit())
                .bind(page.offset()),
            None,
        )
        .await
}

/// Detailed-mode SQL — single SELECT against `information_schema.VIEWS`.
///
/// Eight fields per row, every value a single-column projection from the same
/// `information_schema.VIEWS` row. **No correlated subquery** (views have no
/// parameters). **No DDL reconstruction** (`VIEW_DEFINITION` already returns
/// the SELECT body verbatim — see research Decision 5). The `ALGORITHM` column
/// is deliberately not selected because it is a MariaDB-only addition; touching
/// it would fail on `MySQL` 9 (FR-006).
///
/// `LIMIT` pushes down before the JSON projection, so per-page work scales
/// with `page_size + 1` rows regardless of how many views the schema holds.
const DETAILS: &str = r"
    SELECT
        CAST(v.TABLE_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(v.TABLE_SCHEMA          AS CHAR),
            'definer',             CAST(v.DEFINER               AS CHAR),
            'security',            CAST(v.SECURITY_TYPE         AS CHAR),
            'checkOption',         CAST(v.CHECK_OPTION          AS CHAR),
            'updatable',           (v.IS_UPDATABLE = 'YES'),
            'characterSetClient',  CAST(v.CHARACTER_SET_CLIENT  AS CHAR),
            'collationConnection', CAST(v.COLLATION_CONNECTION  AS CHAR),
            'definition',          CAST(v.VIEW_DEFINITION       AS CHAR)
        ) AS entry
    FROM information_schema.VIEWS v
    WHERE v.TABLE_SCHEMA = ?
      AND (? IS NULL OR LOWER(v.TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY v.TABLE_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of view entries in `database` with details, filtered by `pattern` when given.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn details(
    connection: &MysqlConnection,
    page: Page,
    pattern: Option<&str>,
    database: &str,
) -> Result<Vec<(String, Value)>, SqlError> {
    connection
        .fetch(
            sqlx::query(DETAILS)
                .bind(database)
                .bind(pattern)
                .bind(pattern)
                .bind(page.limit())
                .bind(page.offset()),
            None,
        )
        .await
}
