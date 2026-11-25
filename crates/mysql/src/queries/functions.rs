//! Stored function listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::MysqlConnection;

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(ROUTINE_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
const NAMES: &str = r"
    SELECT CAST(ROUTINE_NAME AS CHAR)
    FROM information_schema.ROUTINES
    WHERE ROUTINE_SCHEMA = ?
      AND ROUTINE_TYPE   = 'FUNCTION'
      AND (? IS NULL OR LOWER(ROUTINE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY ROUTINE_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of function names in `database`, filtered by `pattern` when given.
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

/// Detailed-mode SQL — single SELECT against `information_schema.ROUTINES`.
///
/// `JSON_OBJECT(...)` projects fourteen fields per row. The argument list is
/// assembled by a correlated subquery against `information_schema.PARAMETERS`
/// filtered to the row's `(SPECIFIC_SCHEMA, SPECIFIC_NAME)` and to
/// `ROUTINE_TYPE='FUNCTION'`, `ORDINAL_POSITION > 0` (excluding the synthetic
/// RETURN row at ordinal 0). The reconstructed `definition` produces the
/// canonical `CREATE FUNCTION` text. The five comma-separated `CONCAT`
/// chunks at the top of the `definition` rebuild the canonical
/// `DEFINER=` `` `<user>`@`<host>` `` opener inline; the user portion may
/// itself contain `@` (e.g. `'foo@bar'@'localhost'`), so the host segment is
/// taken after the **last** `@` (`SUBSTRING_INDEX(..., '@', -1)`) and the
/// user is everything before it (`LEFT(..., LENGTH - host_len - 1)`), with
/// embedded backticks doubled in both components. Parameter names in the
/// argument list are backtick-quoted with embedded backticks doubled.
/// `SQL_DATA_ACCESS` is stored with spaces (`'READS SQL DATA'` etc.); the
/// embedded DDL emits the column directly while the structured
/// `sqlDataAccess` field substitutes underscores for programmatic
/// comparison. `QUOTE(...)` on `ROUTINE_COMMENT` produces a properly escaped
/// single-quoted SQL string literal (handles embedded `'` and `\`). The
/// `''` → `null` coercion on `description` mirrors the Postgres detailed-payload contract.
///
/// `LIMIT` pushes down before the JSON projection and the correlated
/// subqueries, so per-page work scales with `page_size + 1` rows regardless
/// of how many functions the schema holds in total.
const DETAILS: &str = r"
    SELECT
        CAST(r.ROUTINE_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(r.ROUTINE_SCHEMA AS CHAR),
            'language',            CAST(COALESCE(NULLIF(r.EXTERNAL_LANGUAGE, ''), r.ROUTINE_BODY) AS CHAR),
            'arguments',           COALESCE((
                SELECT GROUP_CONCAT(
                    CONCAT(CAST(p.PARAMETER_NAME AS CHAR), ' ', CAST(p.DTD_IDENTIFIER AS CHAR))
                    ORDER BY p.ORDINAL_POSITION ASC
                    SEPARATOR ', '
                )
                FROM information_schema.PARAMETERS p
                WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                  AND p.SPECIFIC_NAME   = r.ROUTINE_NAME
                  AND p.ROUTINE_TYPE    = 'FUNCTION'
                  AND p.ORDINAL_POSITION > 0
            ), ''),
            'returnType',          CAST(r.DTD_IDENTIFIER AS CHAR),
            'deterministic',       (r.IS_DETERMINISTIC = 'YES'),
            'sqlDataAccess',       CAST(REPLACE(r.SQL_DATA_ACCESS, ' ', '_') AS CHAR),
            'security',            CAST(r.SECURITY_TYPE AS CHAR),
            'definer',             CAST(r.DEFINER AS CHAR),
            'description',         CASE WHEN r.ROUTINE_COMMENT IS NULL OR r.ROUTINE_COMMENT = ''
                                        THEN NULL ELSE CAST(r.ROUTINE_COMMENT AS CHAR) END,
            'definition',          CONCAT(
                'CREATE DEFINER=`',
                REPLACE(LEFT(r.DEFINER, LENGTH(r.DEFINER) - LENGTH(SUBSTRING_INDEX(r.DEFINER, '@', -1)) - 1), '`', '``'),
                '`@`',
                REPLACE(SUBSTRING_INDEX(r.DEFINER, '@', -1), '`', '``'),
                '`',
                ' FUNCTION ',
                '`', REPLACE(r.ROUTINE_NAME, '`', '``'), '`',
                '(',
                COALESCE((
                    SELECT GROUP_CONCAT(
                        CONCAT('`', REPLACE(p.PARAMETER_NAME, '`', '``'), '` ', CAST(p.DTD_IDENTIFIER AS CHAR))
                        ORDER BY p.ORDINAL_POSITION ASC
                        SEPARATOR ', '
                    )
                    FROM information_schema.PARAMETERS p
                    WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                      AND p.SPECIFIC_NAME   = r.ROUTINE_NAME
                      AND p.ROUTINE_TYPE    = 'FUNCTION'
                      AND p.ORDINAL_POSITION > 0
                ), ''),
                ') RETURNS ',
                CAST(r.DTD_IDENTIFIER AS CHAR),
                CASE WHEN r.IS_DETERMINISTIC = 'YES' THEN ' DETERMINISTIC' ELSE ' NOT DETERMINISTIC' END,
                ' ', CAST(r.SQL_DATA_ACCESS AS CHAR),
                ' SQL SECURITY ', CAST(r.SECURITY_TYPE AS CHAR),
                CASE WHEN r.ROUTINE_COMMENT IS NULL OR r.ROUTINE_COMMENT = '' THEN ''
                     ELSE CONCAT(' COMMENT ', QUOTE(r.ROUTINE_COMMENT)) END,
                ' ',
                CAST(r.ROUTINE_DEFINITION AS CHAR)
            ),
            'sqlMode',             CAST(r.SQL_MODE                AS CHAR),
            'characterSetClient',  CAST(r.CHARACTER_SET_CLIENT    AS CHAR),
            'collationConnection', CAST(r.COLLATION_CONNECTION    AS CHAR),
            'databaseCollation',   CAST(r.DATABASE_COLLATION      AS CHAR)
        ) AS entry
    FROM information_schema.ROUTINES r
    WHERE r.ROUTINE_SCHEMA = ?
      AND r.ROUTINE_TYPE   = 'FUNCTION'
      AND (? IS NULL OR LOWER(r.ROUTINE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY r.ROUTINE_NAME
    LIMIT ? OFFSET ?";

/// Lists one `page` of function entries in `database` with details, filtered by `pattern` when given.
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
