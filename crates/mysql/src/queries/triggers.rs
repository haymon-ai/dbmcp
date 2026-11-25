//! Trigger listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::MysqlConnection;

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(TRIGGER_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
const NAMES: &str = r"
    SELECT CAST(TRIGGER_NAME AS CHAR)
    FROM information_schema.TRIGGERS
    WHERE TRIGGER_SCHEMA = ?
      AND (? IS NULL OR LOWER(TRIGGER_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TRIGGER_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of trigger names in `database`, filtered by `pattern` when given.
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

/// Detailed-mode SQL — single SELECT against `information_schema.TRIGGERS`.
///
/// `JSON_OBJECT(...)` projects ten fields per row. Identifiers in the
/// reconstructed `definition` are backtick-quoted with embedded backticks
/// doubled. The `DEFINER` column stores `user@host` unquoted; the user
/// portion can itself contain `@` (e.g. `'foo@bar'@'localhost'`), so the
/// host is the segment after the **last** `@` and the user is everything
/// before it (`SUBSTRING_INDEX(..., '@', -1)` for host, `LEFT(...)` for
/// user). The five comma-separated `CONCAT` chunks rebuild the canonical
/// `SHOW CREATE TRIGGER` `DEFINER=` `` `<user>`@`<host>` `` opener inline
/// so identifier escaping and the last-`@` split live next to the rest of
/// the projection. `events` is always a single-element array on
/// `MySQL`/`MariaDB` (the engine fires one event per definition);
/// `activationLevel` is always `ROW`. `ORDER BY TRIGGER_NAME` is sufficient —
/// `(TRIGGER_SCHEMA, TRIGGER_NAME)` is the table's primary key, and the
/// `WHERE` clause already pins `TRIGGER_SCHEMA`.
const DETAILS: &str = r"
    SELECT
        CAST(TRIGGER_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(EVENT_OBJECT_SCHEMA AS CHAR),
            'table',               CAST(EVENT_OBJECT_TABLE  AS CHAR),
            'timing',              CAST(ACTION_TIMING       AS CHAR),
            'events',              JSON_ARRAY(CAST(EVENT_MANIPULATION AS CHAR)),
            'activationLevel',     CAST(ACTION_ORIENTATION  AS CHAR),
            'definition',          CONCAT(
                'CREATE DEFINER=`',
                REPLACE(LEFT(DEFINER, LENGTH(DEFINER) - LENGTH(SUBSTRING_INDEX(DEFINER, '@', -1)) - 1), '`', '``'),
                '`@`',
                REPLACE(SUBSTRING_INDEX(DEFINER, '@', -1), '`', '``'),
                '`',
                ' TRIGGER ',
                '`', REPLACE(TRIGGER_NAME, '`', '``'), '`',
                ' ', ACTION_TIMING, ' ', EVENT_MANIPULATION,
                ' ON ',
                '`', REPLACE(EVENT_OBJECT_TABLE,  '`', '``'), '`',
                ' FOR EACH ROW ', ACTION_STATEMENT
            ),
            'sqlMode',             CAST(SQL_MODE                AS CHAR),
            'characterSetClient',  CAST(CHARACTER_SET_CLIENT    AS CHAR),
            'collationConnection', CAST(COLLATION_CONNECTION    AS CHAR),
            'databaseCollation',   CAST(DATABASE_COLLATION      AS CHAR)
        ) AS entry
    FROM information_schema.TRIGGERS
    WHERE TRIGGER_SCHEMA = ?
      AND (? IS NULL OR LOWER(TRIGGER_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TRIGGER_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of detailed trigger entries in `database`, filtered by `pattern`.
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
