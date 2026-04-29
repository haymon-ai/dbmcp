//! MCP tool: `listViews`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::{ListViewsRequest, ListViewsResponse};

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool;

impl ListViewsTool {
    const NAME: &'static str = "listViews";
    const TITLE: &'static str = "List Views";
    const DESCRIPTION: &'static str = r#"List user-defined views in the `public` schema, optionally filtered and/or with full metadata. Materialized views and system-schema views are excluded.

<usecase>
Use when:
- Auditing views across a database (brief mode, default).
- Searching for a view by partial name (pass `search`).
- Inspecting a view's owner, comment, and full SELECT body before querying it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_views` / `pg_class`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on view names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by bare view name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What views are in mydb?" → listViews(database="mydb")
✓ "Find the active-users view" → listViews(search="active")
✓ "What does active_users select?" → listViews(search="active_users", detailed=true)
✗ "Show me the columns of a view" → use listTables with `detailed: true` instead
✗ "List materialized views" → use listMaterializedViews
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of view-name strings, e.g. `["active_orders", "active_users"]`. View names are unique per schema, so no duplicates appear.
Detailed mode: a JSON object keyed by bare view name; each value carries `schema`, `owner`, `description` (or null when no `COMMENT ON VIEW`), and `definition` (the SELECT body verbatim from `pg_views.definition`, with no `CREATE VIEW` wrapper). The view name is the map key only — it is not repeated inside the value.

Detailed mode deliberately omits column metadata (`columns`), the `name` field (already the key), and view-level options (`security_barrier`, `security_invoker`, `WITH CHECK OPTION`). Column shape is recoverable from the `definition` text or via `listTables(detailed=true)` since Postgres exposes views in `pg_class`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `viewname` row order, so a client can switch `detailed` between pages without losing position.
</pagination>"#;
}

impl ToolBase for ListViewsTool {
    type Parameter = ListViewsRequest;
    type Output = ListViewsResponse;
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

impl AsyncTool<PostgresHandler> for ListViewsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_views(params).await
    }
}

/// Brief-mode SQL: `pg_views` scan with `ILIKE` filter on view name.
///
/// `pg_views` already excludes materialized views (those live in `pg_matviews`),
/// and `schemaname = 'public'` keeps system-schema views out. The
/// `($1::text IS NULL OR ...)` trinary lets one statement cover both filtered
/// and unfiltered cases. View names are unique per schema (Postgres enforces
/// this), so `viewname` alone is a stable sort key — no tiebreaker needed.
const BRIEF_SQL: &str = r"
    SELECT viewname
    FROM pg_views
    WHERE schemaname = 'public'
      AND ($1::text IS NULL OR viewname ILIKE '%' || $1 || '%')
    ORDER BY viewname
    LIMIT $2 OFFSET $3";

/// Detailed-mode SQL: per-view `json_build_object` projection.
///
/// `pg_views` excludes materialized views. The `pg_namespace` + `pg_class`
/// joins anchor the relation OID needed by `obj_description`. Postgres defers
/// SELECT-list evaluation past `LIMIT`, so `obj_description` only runs for the
/// page's rows — never the full schema. `pg_views.viewowner` is already a role
/// name, so no `pg_roles` join is needed. View names are unique per schema, so
/// `viewname` alone is a stable sort key.
const DETAILED_SQL: &str = r"
    SELECT
        v.viewname AS name,
        json_build_object(
            'schema',      v.schemaname,
            'owner',       v.viewowner,
            'description', pg_catalog.obj_description(c.oid, 'pg_class'),
            'definition',  v.definition
        ) AS entry
    FROM pg_views v
    JOIN pg_namespace n ON n.nspname = v.schemaname
    JOIN pg_class     c ON c.relname = v.viewname AND c.relnamespace = n.oid
    WHERE v.schemaname = 'public'
      AND ($1::text IS NULL OR v.viewname ILIKE '%' || $1 || '%')
    ORDER BY v.viewname
    LIMIT $2 OFFSET $3";

impl PostgresHandler {
    /// Lists one page of user-defined views, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListViewsRequest,
    ) -> Result<ListViewsResponse, ErrorData> {
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
            return Ok(ListViewsResponse::detailed(
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
        let (views, next_cursor) = pager.paginate(rows);
        Ok(ListViewsResponse::brief(views, next_cursor))
    }
}
