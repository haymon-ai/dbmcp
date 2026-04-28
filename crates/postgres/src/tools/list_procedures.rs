//! MCP tool: `listProcedures`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::{ListProceduresRequest, ListProceduresResponse};

/// Marker type for the `listProcedures` MCP tool.
pub(crate) struct ListProceduresTool;

impl ListProceduresTool {
    const NAME: &'static str = "listProcedures";
    const TITLE: &'static str = "List Procedures";
    const DESCRIPTION: &'static str = r#"List user-defined procedures in the `public` schema, optionally filtered and/or with full metadata. Functions, aggregates, and window functions are excluded.

<usecase>
Use when:
- Auditing procedures across a database (brief mode, default).
- Searching for a procedure by partial name (pass `search`).
- Inspecting a procedure's language, signature, security mode, owner, comment, and full `CREATE PROCEDURE` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_proc` / `information_schema.routines`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on procedure names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by `name(arguments)` instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What procedures are in mydb?" → listProcedures(database="mydb")
✓ "Find the order archival routine" → listProcedures(search="archive")
✓ "What does archive_order do?" → listProcedures(search="archive_order", detailed=true)
✗ "List functions" → use listFunctions instead
✗ "List aggregates" → not supported; aggregates are excluded
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of procedure-name strings, e.g. `["archive_order", "archive_order_history", "archive_order_history"]`. Overloaded procedures appear as one entry per overload (duplicate name strings allowed).
Detailed mode: a JSON object keyed by procedure signature `name(arguments)`; each value carries `schema`, `name`, `language`, `arguments`, `security` (INVOKER/DEFINER), `owner`, `description` (or null when no `COMMENT ON PROCEDURE`), and `definition` (the full `CREATE OR REPLACE PROCEDURE` text). Overloads occupy distinct keys (e.g. `archive_order_history(integer)` vs `archive_order_history(integer, boolean)`). Zero-arg procedures key as `name()` — the parens are always present so the key shape stays uniform.

Detailed mode deliberately omits the `listFunctions`-only fields `returnType`, `volatility`, `strict`, and `parallelSafety`: procedures don't return a value, `pg_proc.provolatile` / `proisstrict` are not user-settable for procedures, and `proparallel` carries no procedure-level guarantee in PostgreSQL.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `(proname, oid)` row order, so a client can switch `detailed` between pages without losing position.
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

impl AsyncTool<PostgresHandler> for ListProceduresTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_procedures(params).await
    }
}

/// Brief-mode SQL: `pg_proc` scan with `ILIKE` filter on procedure name.
///
/// `n.nspname = 'public'` plus `p.prokind = 'p'` keep functions / aggregates /
/// window functions out of the result. The `($1::text IS NULL OR ...)` trinary
/// lets one statement cover both filtered and unfiltered cases. `(p.proname,
/// p.oid)` is the sort key — `oid` is the unique tiebreaker across overloaded
/// names so `OFFSET` pagination is deterministic.
const BRIEF_SQL: &str = r"
    SELECT p.proname
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND p.prokind = 'p'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

/// Detailed-mode SQL: per-procedure `json_build_object` projection.
///
/// `n.nspname = 'public'` and `p.prokind = 'p'` filter to user-defined
/// procedures in the `public` schema. Three small lookup joins (`pg_namespace`,
/// `pg_language`, `pg_roles`) supply the language and owner names. Postgres
/// defers SELECT-list evaluation past `LIMIT`, so the expensive `pg_get_*`
/// projections (`pg_get_functiondef`, `pg_get_function_arguments`) and
/// `obj_description` only run for the page's rows — never the full schema.
///
/// A `CROSS JOIN LATERAL` materialises `pg_get_function_arguments(p.oid)` into
/// `args.text` so it is computed once per row and reused both in the keyed
/// signature `name(args)` and in the JSON `arguments` field — no double-call.
///
/// `prosecdef` is a boolean. Procedure-only fields (`returnType`, `volatility`,
/// `strict`, `parallelSafety`) are deliberately absent — see DESCRIPTION above.
const DETAILED_SQL: &str = r"
    SELECT
        p.proname || '(' || args.text || ')' AS name,
        json_build_object(
            'schema',      'public',
            'name',        p.proname,
            'language',    l.lanname,
            'arguments',   args.text,
            'security',    CASE WHEN p.prosecdef THEN 'DEFINER' ELSE 'INVOKER' END,
            'owner',       r.rolname,
            'description', pg_catalog.obj_description(p.oid, 'pg_proc'),
            'definition',  pg_get_functiondef(p.oid)
        ) AS entry
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    JOIN pg_language  l ON l.oid = p.prolang
    JOIN pg_roles     r ON r.oid = p.proowner
    CROSS JOIN LATERAL (SELECT pg_get_function_arguments(p.oid) AS text) args
    WHERE n.nspname = 'public'
      AND p.prokind = 'p'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

impl PostgresHandler {
    /// Lists one page of user-defined procedures, optionally filtered and/or detailed.
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
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pattern = search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pager = Pager::new(cursor, self.config.page_size);

        if detailed {
            let rows: Vec<(String, sqlx::types::Json<serde_json::Value>)> = self
                .connection
                .fetch(
                    sqlx::query(DETAILED_SQL)
                        .bind(pattern)
                        .bind(pager.limit())
                        .bind(pager.offset()),
                    database,
                )
                .await?;
            let (rows, next_cursor) = pager.paginate(rows);
            return Ok(ListProceduresResponse::detailed(
                rows.into_iter().map(|(key, json)| (key, json.0)).collect(),
                next_cursor,
            ));
        }

        let rows: Vec<String> = self
            .connection
            .fetch_scalar(
                sqlx::query(BRIEF_SQL)
                    .bind(pattern)
                    .bind(pager.limit())
                    .bind(pager.offset()),
                database,
            )
            .await?;
        let (procedures, next_cursor) = pager.paginate(rows);
        Ok(ListProceduresResponse::brief(procedures, next_cursor))
    }
}
