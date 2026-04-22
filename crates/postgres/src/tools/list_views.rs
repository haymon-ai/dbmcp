//! MCP tool: `listViews`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListViewsRequest, ListViewsResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool;

impl ListViewsTool {
    const NAME: &'static str = "listViews";
    const TITLE: &'static str = "List Views";
    const DESCRIPTION: &'static str = r#"List all views in a database.

<usecase>
Use when:
- Exploring a database to find defined views alongside tables
- Verifying a view exists before querying it
- The user asks what views are in a database
</usecase>

<examples>
✓ "What views are in the mydb database?" → listViews(database="mydb")
✓ "Does an active_users view exist?" → listViews to check
✗ "Show me the columns of a view" → use getTableSchema instead
✗ "List materialized views" → use listMaterializedViews
</examples>

<what_it_returns>
A sorted JSON array of view name strings in the `public` schema.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
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

impl PostgresHandler {
    /// Lists one page of views in the `public` schema of a database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest { database, cursor }: ListViewsRequest,
    ) -> Result<ListViewsResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(validate_ident)
            .transpose()?;

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT viewname
            FROM pg_views
            WHERE schemaname = 'public'
            ORDER BY viewname
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), database).await?;
        let (views, next_cursor) = pager.finalize(rows);

        Ok(ListViewsResponse { views, next_cursor })
    }
}
