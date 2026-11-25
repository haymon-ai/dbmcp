//! MCP tool: `listViews`.

use dbmcp_server::listing::{self, Cursor};

use super::prelude::*;
use crate::queries;
use crate::types::{ListEntriesResponse, ListViewsRequest};

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

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ListViewsTool<PINNED> {
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
        Some(
            if PINNED {
                DESCRIPTION_PINNED
            } else {
                DESCRIPTION_UNPINNED
            }
            .into(),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl<const PINNED: bool> AsyncTool<MysqlHandler> for ListViewsTool<PINNED> {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_views(params.database, params.cursor, params.search, params.detailed)
            .await
    }
}

impl MysqlHandler {
    /// Lists one page of views, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_views(
        &self,
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.connection.default_database_name());

        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::views::names(&self.connection, page, pattern, database).await,
            async |page, pattern| queries::views::details(&self.connection, page, pattern, database).await,
        )
        .await
        .map_err(Into::into)
    }
}
