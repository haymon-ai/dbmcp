//! MCP tool: `listFunctions`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::{ListFunctionsRequest, ListFunctionsResponse};

/// Marker type for the `listFunctions` MCP tool.
pub(crate) struct ListFunctionsTool;

impl ListFunctionsTool {
    const NAME: &'static str = "listFunctions";
    const TITLE: &'static str = "List Functions";
    const DESCRIPTION: &'static str = r#"List user-defined functions in the `public` schema, optionally filtered and/or with full metadata. Aggregates, window functions, and procedures are excluded.

<usecase>
Use when:
- Auditing functions across a database (brief mode, default).
- Searching for a function by partial name (pass `search`).
- Inspecting a function's language, signature, return type, volatility, strictness, security mode, parallel-safety, owner, comment, and full `CREATE FUNCTION` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_proc` / `information_schema.routines`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on function names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by `name(arguments)` instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What functions are in mydb?" → listFunctions(database="mydb")
✓ "Find the order-total calculation" → listFunctions(search="order")
✓ "What does calc_order_total do?" → listFunctions(search="calc_order_total", detailed=true)
✗ "List stored procedures" → use listProcedures instead
✗ "List aggregates" → not supported; aggregates are excluded
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of function-name strings, e.g. `["audit_user_login", "calc_order_subtotal", "calc_order_total", "calc_order_total"]`. Overloaded functions appear as one entry per overload (duplicate name strings allowed).
Detailed mode: a JSON object keyed by function signature `name(arguments)`; each value carries `schema`, `name`, `language`, `arguments`, `returnType`, `volatility` (IMMUTABLE/STABLE/VOLATILE), `strict` (boolean), `security` (INVOKER/DEFINER), `parallelSafety` (SAFE/RESTRICTED/UNSAFE), `owner`, `description` (or null when no `COMMENT ON FUNCTION`), and `definition` (the full `CREATE OR REPLACE FUNCTION` text). Overloads occupy distinct keys (e.g. `calc_total(integer)` vs `calc_total(integer, numeric)`).
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `(proname, oid)` row order, so a client can switch `detailed` between pages without losing position.
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

impl AsyncTool<PostgresHandler> for ListFunctionsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_functions(params).await
    }
}

/// Brief-mode SQL: `pg_proc` scan with `ILIKE` filter on function name.
///
/// `n.nspname = 'public'` plus `p.prokind = 'f'` keep aggregates / window
/// functions / procedures out of the result. The `($1::text IS NULL OR ...)`
/// trinary lets one statement cover both filtered and unfiltered cases.
/// `(p.proname, p.oid)` is the sort key — `oid` is the unique tiebreaker
/// across overloaded names so `OFFSET` pagination is deterministic.
const BRIEF_SQL: &str = r"
    SELECT p.proname
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND p.prokind = 'f'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

/// Detailed-mode SQL: per-function `json_build_object` projection.
///
/// `n.nspname = 'public'` and `p.prokind = 'f'` filter to user-defined
/// functions in the `public` schema. Three small lookup joins (`pg_namespace`,
/// `pg_language`, `pg_roles`) supply the language and owner names. Postgres
/// defers SELECT-list evaluation past `LIMIT`, so the expensive `pg_get_*`
/// projections (`pg_get_functiondef`, `pg_get_function_arguments`,
/// `pg_get_function_result`) and `obj_description` only run for the page's
/// rows — never the full schema.
///
/// A `CROSS JOIN LATERAL` materialises `pg_get_function_arguments(p.oid)`
/// into `args.text` so it is computed once per row and reused both in the
/// keyed signature `name(args)` and in the JSON `arguments` field — no
/// double-call, no Rust-side json lookup.
///
/// `provolatile`, `proisstrict`, `prosecdef`, `proparallel` are stable
/// `pg_proc` columns. `provolatile` is `'i'`/`'s'`/`'v'`; `proparallel` is
/// `'s'`/`'r'`/`'u'`; `prosecdef` is a boolean.
const DETAILED_SQL: &str = r"
    SELECT
        p.proname || '(' || args.text || ')' AS name,
        json_build_object(
            'schema',         'public',
            'name',           p.proname,
            'language',       l.lanname,
            'arguments',      args.text,
            'returnType',     pg_get_function_result(p.oid),
            'volatility',     CASE p.provolatile
                                  WHEN 'i' THEN 'IMMUTABLE'
                                  WHEN 's' THEN 'STABLE'
                                  WHEN 'v' THEN 'VOLATILE'
                              END,
            'strict',         p.proisstrict,
            'security',       CASE WHEN p.prosecdef THEN 'DEFINER' ELSE 'INVOKER' END,
            'parallelSafety', CASE p.proparallel
                                  WHEN 's' THEN 'SAFE'
                                  WHEN 'r' THEN 'RESTRICTED'
                                  WHEN 'u' THEN 'UNSAFE'
                              END,
            'owner',          r.rolname,
            'description',    pg_catalog.obj_description(p.oid, 'pg_proc'),
            'definition',     pg_get_functiondef(p.oid)
        ) AS entry
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    JOIN pg_language  l ON l.oid = p.prolang
    JOIN pg_roles     r ON r.oid = p.proowner
    CROSS JOIN LATERAL (SELECT pg_get_function_arguments(p.oid) AS text) args
    WHERE n.nspname = 'public'
      AND p.prokind = 'f'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

impl PostgresHandler {
    /// Lists one page of user-defined functions, optionally filtered and/or detailed.
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
            return Ok(ListFunctionsResponse::detailed(
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
        let (functions, next_cursor) = pager.paginate(rows);
        Ok(ListFunctionsResponse::brief(functions, next_cursor))
    }
}
