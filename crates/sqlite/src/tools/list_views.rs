//! MCP tool: `listViews`.

use dbmcp_server::listing;
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::types::ListViewsRequest;

const NAME: &str = "listViews";
const TITLE: &str = "List Views";
const DESCRIPTION: &str = include_str!("../../assets/tools/list_views.md");

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool;

impl ToolBase for ListViewsTool {
    type Parameter = ListViewsRequest;
    type Output = ListEntriesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION.into())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        None
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
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

impl AsyncTool<SqliteHandler> for ListViewsTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_views(params).await
    }
}

/// Lists one page of user view names, sorted by name.
const SQL: &str = r"
    SELECT name
    FROM sqlite_schema
    WHERE type = 'view' AND name NOT LIKE 'sqlite_%'
    ORDER BY name
    LIMIT ?1 OFFSET ?2";

impl SqliteHandler {
    /// Lists one page of views in the connected database.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest { cursor }: ListViewsRequest,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let (views, next_cursor) = listing::paginate(cursor, self.config.page_size, async |page| {
            self.connection
                .fetch_scalar(sqlx::query(SQL).bind(page.limit()).bind(page.offset()), None)
                .await
        })
        .await?;

        Ok(ListEntriesResponse::brief(views, next_cursor))
    }
}
