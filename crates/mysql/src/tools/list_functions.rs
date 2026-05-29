//! MCP tool: `listFunctions`.

use dbmcp_server::pagination::{Cursor, Pager};
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::types::ListFunctionsRequest;

const NAME: &str = "listFunctions";
const TITLE: &str = "List Functions";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/list_functions/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/list_functions/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// Marker type for the `listFunctions` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedListFunctionsTool;

impl ToolBase for PinnedListFunctionsTool {
    type Parameter = ListFunctionsRequest;
    type Output = ListEntriesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_PINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for PinnedListFunctionsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_functions(None, params.cursor, params.search, params.detailed)
            .await
    }
}

/// Marker type for the `listFunctions` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedListFunctionsTool;

impl ToolBase for UnpinnedListFunctionsTool {
    type Parameter = ListFunctionsRequest;
    type Output = ListEntriesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_UNPINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(false))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for UnpinnedListFunctionsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_functions(params.database, params.cursor, params.search, params.detailed)
            .await
    }
}

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(ROUTINE_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
const BRIEF_SQL: &str = r"
    SELECT CAST(ROUTINE_NAME AS CHAR)
    FROM information_schema.ROUTINES
    WHERE ROUTINE_SCHEMA = ?
      AND ROUTINE_TYPE   = 'FUNCTION'
      AND (? IS NULL OR LOWER(ROUTINE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY ROUTINE_NAME
    LIMIT ? OFFSET ?";

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
const DETAILED_SQL: &str = r"
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

impl MysqlHandler {
    /// Lists one page of stored functions, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_functions(
        &self,
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.connection.default_database_name());

        let pattern = search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pager = Pager::new(cursor, self.config.page_size);

        if detailed {
            let rows: Vec<(String, sqlx::types::Json<serde_json::Value>)> = self
                .connection
                .fetch(
                    sqlx::query(DETAILED_SQL)
                        .bind(database)
                        .bind(pattern)
                        .bind(pattern)
                        .bind(pager.limit())
                        .bind(pager.offset()),
                    None,
                )
                .await?;
            let (rows, next_cursor) = pager.paginate(rows);
            return Ok(ListEntriesResponse::detailed(
                rows.into_iter().map(|(name, json)| (name, json.0)).collect(),
                next_cursor,
            ));
        }

        let rows: Vec<String> = self
            .connection
            .fetch_scalar(
                sqlx::query(BRIEF_SQL)
                    .bind(database)
                    .bind(pattern)
                    .bind(pattern)
                    .bind(pager.limit())
                    .bind(pager.offset()),
                None,
            )
            .await?;
        let (functions, next_cursor) = pager.paginate(rows);
        Ok(ListEntriesResponse::brief(functions, next_cursor))
    }
}
