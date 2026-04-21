//! MCP tool: `listMaterializedViews`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ListMaterializedViewsRequest, ListMaterializedViewsResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listMaterializedViews` MCP tool.
pub(crate) struct ListMaterializedViewsTool;

impl ListMaterializedViewsTool {
    const NAME: &'static str = "listMaterializedViews";
    const TITLE: &'static str = "List Materialized Views";
    const DESCRIPTION: &'static str = r#"List all materialized views in the `public` schema of a PostgreSQL database. Unlike regular views, materialized views store their results physically and must be refreshed explicitly.

<usecase>
Use when:
- Exploring a database for stored aggregates that may be stale
- Auditing which materialized views require refresh scheduling
- The user asks what materialized views exist
</usecase>

<examples>
✓ "What materialized views are in mydb?" → listMaterializedViews(database="mydb")
✓ "Does an mv_recent_orders materialized view exist?" → listMaterializedViews to check
✗ "List regular views" → use listViews instead
</examples>

<what_it_returns>
A sorted JSON array of materialized-view name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
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

impl PostgresHandler {
    /// Lists one page of materialized views in the `public` schema.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_materialized_views(
        &self,
        ListMaterializedViewsRequest { database, cursor }: ListMaterializedViewsRequest,
    ) -> Result<ListMaterializedViewsResponse, ErrorData> {
        let db = Some(database.trim()).filter(|s| !s.is_empty());
        if let Some(name) = db {
            validate_ident(name)?;
        }

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT matviewname
            FROM pg_matviews
            WHERE schemaname = 'public'
            ORDER BY matviewname
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), db).await?;
        let (materialized_views, next_cursor) = pager.finalize(rows);

        Ok(ListMaterializedViewsResponse {
            materialized_views,
            next_cursor,
        })
    }
}
