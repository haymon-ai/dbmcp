//! MCP tool: `listMaterializedViews`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::{ListMaterializedViewsRequest, ListMaterializedViewsResponse};

/// Marker type for the `listMaterializedViews` MCP tool.
pub(crate) struct ListMaterializedViewsTool;

impl ListMaterializedViewsTool {
    const NAME: &'static str = "listMaterializedViews";
    const TITLE: &'static str = "List Materialized Views";
    const DESCRIPTION: &'static str = r#"List materialized views in the `public` schema, optionally filtered and/or with full metadata. Unlike regular views, materialized views store their results physically and must be refreshed explicitly. Regular views and system-schema matviews are excluded.

<usecase>
Use when:
- Auditing materialized views across a database (brief mode, default).
- Searching for a matview by partial name (pass `search`).
- Inspecting a matview's owner, comment, full SELECT body, populated state, and index presence before querying or refreshing it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_matviews` / `pg_class`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on matview names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by bare matview name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What materialized views are in mydb?" → listMaterializedViews(database="mydb")
✓ "Find the recent-orders matview" → listMaterializedViews(search="orders")
✓ "What does mv_orders_by_region compute?" → listMaterializedViews(search="mv_orders_by_region", detailed=true)
✓ "Has the cache matview ever been refreshed?" → listMaterializedViews(search="cache", detailed=true) — read `populated`
✓ "Which matviews can I refresh concurrently?" → listMaterializedViews(detailed=true) — read `indexed` (CONCURRENTLY additionally needs a unique index)
✗ "List regular views" → use listViews instead
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of matview-name strings, e.g. `["mv_archived_orders", "mv_recent_orders"]`. Matview names are unique per schema, so no duplicates appear.
Detailed mode: a JSON object keyed by bare matview name; each value carries:
- `schema` — schema name (always `"public"` in this build).
- `owner` — owning role's name from `pg_matviews.matviewowner`.
- `description` — `COMMENT ON MATERIALIZED VIEW` text, or `null` when no comment.
- `definition` — the SELECT body verbatim from `pg_matviews.definition`, with no `CREATE MATERIALIZED VIEW` wrapper.
- `populated` — `true` once the matview has been refreshed at least once. `false` for matviews created `WITH NO DATA` and never refreshed; querying such a matview returns zero rows until `REFRESH MATERIALIZED VIEW` runs.
- `indexed` — `true` when at least one index exists on the matview. `REFRESH MATERIALIZED VIEW CONCURRENTLY` additionally requires a unique index; this tool reports the broader has-any-index signal.

The matview name is the map key only — it is not repeated inside the value. Detailed mode deliberately omits column metadata (`columns`), `tablespace`, storage parameters, and unique-index detection. Column shape is recoverable from the `definition` text or via `listTables(detailed=true)` since Postgres exposes matviews in `pg_class`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `matviewname` row order, so a client can switch `detailed` between pages without losing position.
</pagination>"#;
}

impl ToolBase for ListMaterializedViewsTool {
    type Parameter = ListMaterializedViewsRequest;
    type Output = ListMaterializedViewsResponse;
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

impl AsyncTool<PostgresHandler> for ListMaterializedViewsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_materialized_views(params).await
    }
}

/// Brief-mode SQL: `pg_matviews` scan with `ILIKE` filter on matview name.
///
/// `pg_matviews` already filters to `pg_class.relkind = 'm'`, and
/// `schemaname = 'public'` keeps system-schema matviews out. The
/// `($1::text IS NULL OR ...)` trinary lets one statement cover both filtered
/// and unfiltered cases. Matview names are unique per schema (Postgres enforces
/// this via `pg_class`'s unique index on `(relname, relnamespace)`), so
/// `matviewname` alone is a stable sort key — no tiebreaker needed.
const BRIEF_SQL: &str = r"
    SELECT matviewname
    FROM pg_matviews
    WHERE schemaname = 'public'
      AND ($1::text IS NULL OR matviewname ILIKE '%' || $1 || '%')
    ORDER BY matviewname
    LIMIT $2 OFFSET $3";

/// Detailed-mode SQL: per-matview `json_build_object` projection.
///
/// `pg_matviews` already filters to `relkind = 'm'`. The `pg_namespace` +
/// `pg_class` joins anchor the relation OID needed by `obj_description`.
/// Postgres defers SELECT-list evaluation past `LIMIT`, so `obj_description`
/// only runs for the page's rows — never the full schema.
/// `pg_matviews.matviewowner` is already a role name, so no `pg_roles` join is
/// needed. `populated` and `indexed` are projected directly from
/// `pg_matviews.ispopulated` / `pg_matviews.hasindexes`. Matview names are
/// unique per schema, so `matviewname` alone is a stable sort key.
const DETAILED_SQL: &str = r"
    SELECT
        mv.matviewname AS name,
        json_build_object(
            'schema',      mv.schemaname,
            'owner',       mv.matviewowner,
            'description', pg_catalog.obj_description(c.oid, 'pg_class'),
            'definition',  mv.definition,
            'populated',   mv.ispopulated,
            'indexed',     mv.hasindexes
        ) AS entry
    FROM pg_matviews mv
    JOIN pg_namespace n ON n.nspname = mv.schemaname
    JOIN pg_class     c ON c.relname = mv.matviewname AND c.relnamespace = n.oid
    WHERE mv.schemaname = 'public'
      AND ($1::text IS NULL OR mv.matviewname ILIKE '%' || $1 || '%')
    ORDER BY mv.matviewname
    LIMIT $2 OFFSET $3";

impl PostgresHandler {
    /// Lists one page of user-defined materialized views, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_materialized_views(
        &self,
        ListMaterializedViewsRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListMaterializedViewsRequest,
    ) -> Result<ListMaterializedViewsResponse, ErrorData> {
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
            return Ok(ListMaterializedViewsResponse::detailed(
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
        let (materialized_views, next_cursor) = pager.paginate(rows);
        Ok(ListMaterializedViewsResponse::brief(materialized_views, next_cursor))
    }
}
