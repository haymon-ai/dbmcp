//! MCP tool: `listProcedures`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::types::{ListProceduresRequest, ListProceduresResponse};

/// Marker type for the `listProcedures` MCP tool.
pub(crate) struct ListProceduresTool;

impl ListProceduresTool {
    const NAME: &'static str = "listProcedures";
    const TITLE: &'static str = "List Procedures";
    const DESCRIPTION: &'static str = r#"List user-defined stored procedures in a database, optionally filtered and/or with full metadata. Stored functions and loadable UDFs (`mysql.func`) are excluded.

<usecase>
Use when:
- Auditing stored procedures across a database (brief mode, default).
- Searching for a procedure by partial name (pass `search`).
- Inspecting a procedure's language, parameter list (with `IN`/`OUT`/`INOUT` modes), determinism, SQL-data-access classification, security mode, definer, comment, session context, and full reconstructed `CREATE PROCEDURE` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.ROUTINES` / `information_schema.PARAMETERS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on procedure names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by procedure name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What procedures are in the mydb database?" → listProcedures(database="mydb")
✓ "Find the order archival routine" → listProcedures(search="archive")
✓ "What does archive_order do?" → listProcedures(search="archive_order", detailed=true)
✗ "List functions" → use listFunctions instead
✗ "List loadable UDFs from mysql.func" → not supported; only routines in information_schema.ROUTINES are returned
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of procedure-name strings, e.g. `["archive_order", "archive_order_history", "purge_order_archive", "touch_post"]`.
Detailed mode: a JSON object keyed by bare procedure name (MySQL/MariaDB do not allow procedure overloading, so no signature suffix is needed); each value carries `schema`, `language` (typically `"SQL"`; MariaDB external-language procedures report the external language name), `arguments` (comma-separated `MODE name type` triples from `information_schema.PARAMETERS` — `MODE` is one of `IN`, `OUT`, `INOUT`; empty string for zero-parameter procedures), `deterministic` (boolean), `sqlDataAccess` (one of `CONTAINS_SQL`, `NO_SQL`, `READS_SQL_DATA`, `MODIFIES_SQL_DATA`), `security` (`INVOKER` or `DEFINER`), `definer` (`user@host`), `description` (the `COMMENT` text or `null` when no comment was set — the empty string MySQL stores is coerced to JSON `null`), `definition` (the canonical reconstructed `CREATE PROCEDURE` text including `DEFINER=` in `` `user`@`host` `` form; no `RETURNS` clause — procedures have no return type), `sqlMode`, `characterSetClient`, `collationConnection`, and `databaseCollation`. Versus the Postgres `listProcedures` detailed payload: `volatility`, `parallelSafety`, `strict`, and `returnType` are intentionally absent (no MySQL/MariaDB analogues for the first three; procedures have no return type for the fourth — the Postgres-side payload also omits all four), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), keys are bare names rather than `name(arguments)` (no overloads possible), the four session-context fields (`sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`) are MySQL/MariaDB-only additions, and `deterministic` plus `sqlDataAccess` are MySQL/MariaDB-only additions versus the Postgres `listProcedures` detailed payload (which omits both because Postgres procedures have no equivalent attributes).
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>"#;
}

impl ToolBase for ListProceduresTool {
    type Parameter = ListProceduresRequest;
    type Output = ListProceduresResponse;
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

impl AsyncTool<MysqlHandler> for ListProceduresTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_procedures(params).await
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
      AND ROUTINE_TYPE   = 'PROCEDURE'
      AND (? IS NULL OR LOWER(ROUTINE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY ROUTINE_NAME
    LIMIT ? OFFSET ?";

/// Detailed-mode SQL — single SELECT against `information_schema.ROUTINES`.
///
/// `JSON_OBJECT(...)` projects thirteen fields per row. The parameter list is
/// assembled by a correlated subquery against `information_schema.PARAMETERS`
/// filtered to the row's `(SPECIFIC_SCHEMA, SPECIFIC_NAME)` and to
/// `ROUTINE_TYPE='PROCEDURE'`. Procedures (unlike functions) have no synthetic
/// RETURN row at ordinal 0, so no `ORDINAL_POSITION > 0` filter is needed —
/// every parameter row is real. Each parameter token is rendered as
/// `MODE name type`, where `MODE` is one of `IN`, `OUT`, `INOUT` and is taken
/// verbatim from `PARAMETER_MODE`.
///
/// The reconstructed `definition` produces the canonical `CREATE PROCEDURE`
/// text. The five comma-separated `CONCAT` chunks at the top of the
/// `definition` rebuild the canonical `DEFINER=` `` `<user>`@`<host>` `` opener
/// inline; the user portion may itself contain `@` (e.g.
/// `'foo@bar'@'localhost'`), so the host segment is taken after the **last**
/// `@` (`SUBSTRING_INDEX(..., '@', -1)`) and the user is everything before it
/// (`LEFT(..., LENGTH - host_len - 1)`), with embedded backticks doubled in
/// both components. Parameter names in the argument list are backtick-quoted
/// with embedded backticks doubled. There is NO `RETURNS <type>` clause —
/// procedures have no return type. `SQL_DATA_ACCESS` is stored with spaces
/// (`'READS SQL DATA'` etc.); the embedded DDL emits the column directly while
/// the structured `sqlDataAccess` field substitutes underscores for
/// programmatic comparison. `QUOTE(...)` on `ROUTINE_COMMENT` produces a
/// properly escaped single-quoted SQL string literal (handles embedded `'` and
/// `\`). The `''` → `null` coercion on `description` mirrors the
/// Postgres detailed-payload contract.
///
/// `LIMIT` pushes down before the JSON projection and the correlated
/// subqueries, so per-page work scales with `page_size + 1` rows regardless
/// of how many procedures the schema holds in total.
const DETAILED_SQL: &str = r"
    SELECT
        CAST(r.ROUTINE_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(r.ROUTINE_SCHEMA AS CHAR),
            'language',            CAST(COALESCE(NULLIF(r.EXTERNAL_LANGUAGE, ''), r.ROUTINE_BODY) AS CHAR),
            'arguments',           COALESCE((
                SELECT GROUP_CONCAT(
                    CONCAT(
                        CAST(p.PARAMETER_MODE AS CHAR), ' ',
                        CAST(p.PARAMETER_NAME AS CHAR), ' ',
                        CAST(p.DTD_IDENTIFIER  AS CHAR)
                    )
                    ORDER BY p.ORDINAL_POSITION ASC
                    SEPARATOR ', '
                )
                FROM information_schema.PARAMETERS p
                WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                  AND p.SPECIFIC_NAME   = r.ROUTINE_NAME
                  AND p.ROUTINE_TYPE    = 'PROCEDURE'
            ), ''),
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
                ' PROCEDURE ',
                '`', REPLACE(r.ROUTINE_NAME, '`', '``'), '`',
                '(',
                COALESCE((
                    SELECT GROUP_CONCAT(
                        CONCAT(
                            CAST(p.PARAMETER_MODE AS CHAR), ' ',
                            '`', REPLACE(p.PARAMETER_NAME, '`', '``'), '` ',
                            CAST(p.DTD_IDENTIFIER AS CHAR)
                        )
                        ORDER BY p.ORDINAL_POSITION ASC
                        SEPARATOR ', '
                    )
                    FROM information_schema.PARAMETERS p
                    WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                      AND p.SPECIFIC_NAME   = r.ROUTINE_NAME
                      AND p.ROUTINE_TYPE    = 'PROCEDURE'
                ), ''),
                ')',
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
      AND r.ROUTINE_TYPE   = 'PROCEDURE'
      AND (? IS NULL OR LOWER(r.ROUTINE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY r.ROUTINE_NAME
    LIMIT ? OFFSET ?";

impl MysqlHandler {
    /// Lists one page of stored procedures, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_procedures(
        &self,
        ListProceduresRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListProceduresRequest,
    ) -> Result<ListProceduresResponse, ErrorData> {
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
            return Ok(ListProceduresResponse::detailed(
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
        let (procedures, next_cursor) = pager.paginate(rows);
        Ok(ListProceduresResponse::brief(procedures, next_cursor))
    }
}
