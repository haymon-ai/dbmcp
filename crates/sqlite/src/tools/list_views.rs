//! MCP tool: `listViews`.

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::ListViewsResponse;

use super::prelude::*;
use crate::types::ListViewsRequest;

const NAME: &str = "listViews";
const TITLE: &str = "List Views";
const DESCRIPTION: &str = include_str!("../../assets/tools/list_views.md");

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool;

impl ToolBase for ListViewsTool {
    type Parameter = ListViewsRequest;
    type Output = ListViewsResponse;
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

impl SqliteHandler {
    /// Lists one page of views in the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest { cursor }: ListViewsRequest,
    ) -> Result<ListViewsResponse, ErrorData> {
        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT name
            FROM sqlite_schema
            WHERE type = 'view' AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (views, next_cursor) = pager.paginate(rows);

        Ok(ListViewsResponse::brief(views, next_cursor))
    }
}
