//! MCP tool: `listViews`.

use dbmcp_server::pagination::{Cursor, Pager};

use super::prelude::*;
use crate::types::{ListViewsResponse, PinnedListViewsRequest, UnpinnedListViewsRequest};

const NAME: &str = "listViews";
const TITLE: &str = "List Views";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/list_views/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/list_views/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// Marker type for the `listViews` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedListViewsTool;

impl ToolBase for PinnedListViewsTool {
    type Parameter = PinnedListViewsRequest;
    type Output = ListViewsResponse;
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
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for PinnedListViewsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_views(None, params.cursor, params.search, params.detailed)
            .await
    }
}

/// Marker type for the `listViews` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedListViewsTool;

impl ToolBase for UnpinnedListViewsTool {
    type Parameter = UnpinnedListViewsRequest;
    type Output = ListViewsResponse;
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
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for UnpinnedListViewsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_views(
                params.database,
                params.inner.cursor,
                params.inner.search,
                params.inner.detailed,
            )
            .await
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
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
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
