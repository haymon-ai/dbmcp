//! MCP tool: `listFunctions`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::ListFunctionsResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::types::ListFunctionsRequest;

/// Marker type for the `listFunctions` MCP tool.
pub(crate) struct ListFunctionsTool;

impl ListFunctionsTool {
    const NAME: &'static str = "listFunctions";
    const TITLE: &'static str = "List Functions";
    const DESCRIPTION: &'static str = r#"List user-defined SQL functions in a database, optionally filtered and/or with full metadata. Loadable UDFs (`mysql.func`) and stored procedures are excluded.

<usecase>
Use when:
- Auditing stored functions across a database (brief mode, default).
- Searching for a function by partial name (pass `search`).
- Inspecting a function's language, signature, return type, determinism, SQL-data-access classification, security mode, definer, comment, session context, and full reconstructed `CREATE FUNCTION` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.ROUTINES` / `information_schema.PARAMETERS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on function names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by function name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What functions are in the mydb database?" → listFunctions(database="mydb")
✓ "Find the order-total calculation" → listFunctions(search="order")
✓ "What does calc_order_total do?" → listFunctions(search="calc_order_total", detailed=true)
✗ "List stored procedures" → use listProcedures instead
✗ "List loadable UDFs from mysql.func" → not supported; only routines in information_schema.ROUTINES are returned
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of function-name strings, e.g. `["calc_order_subtotal", "calc_order_total", "double_it"]`.
Detailed mode: a JSON object keyed by bare function name (MySQL/MariaDB do not allow function overloading, so no signature suffix is needed); each value carries `schema`, `language` (typically `"SQL"`; MariaDB external-language functions report the external language name), `arguments` (comma-separated `name type` pairs from `information_schema.PARAMETERS`, empty string for zero-parameter functions), `returnType` (full `DTD_IDENTIFIER` including length/precision/unsigned/enum/set members), `deterministic` (boolean), `sqlDataAccess` (one of `CONTAINS_SQL`, `NO_SQL`, `READS_SQL_DATA`, `MODIFIES_SQL_DATA`), `security` (`INVOKER` or `DEFINER`), `definer` (`user@host`), `description` (the `COMMENT` text or `null` when no comment was set — the empty string MySQL stores is coerced to JSON `null`), `definition` (the canonical reconstructed `CREATE FUNCTION` text including `DEFINER=` in `` `user`@`host` `` form), `sqlMode`, `characterSetClient`, `collationConnection`, and `databaseCollation`. Versus the Postgres detailed payload: `volatility`, `parallelSafety`, and `strict` are intentionally absent (no MySQL/MariaDB analogues), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), keys are bare names rather than `name(arguments)` (no overloads possible), and the four session-context fields (`sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`) are MySQL/MariaDB-only additions.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>"#;
}

impl ToolBase for ListFunctionsTool {
    type Parameter = ListFunctionsRequest;
    type Output = ListFunctionsResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }
}

impl AsyncTool<MysqlHandler> for ListFunctionsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_functions(params).await
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
        ListFunctionsRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListFunctionsRequest,
    ) -> Result<ListFunctionsResponse, ErrorData> {
        let database = validate_ident(
            database
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.connection.default_database_name()),
        )?;

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
            return Ok(ListFunctionsResponse::detailed(
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
        Ok(ListFunctionsResponse::brief(functions, next_cursor))
    }
}
