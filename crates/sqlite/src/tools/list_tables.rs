//! MCP tool: `listTables`.

use dbmcp_server::listing;

use super::prelude::*;
use crate::queries;
use crate::types::{ListEntriesResponse, ListTablesRequest};

const NAME: &str = "listTables";
const TITLE: &str = "List Tables";
const DESCRIPTION: &str = include_str!("../../assets/tools/list_tables.md");

/// Marker type for the `listTables` MCP tool.
pub(crate) struct ListTablesTool;

impl ToolBase for ListTablesTool {
    type Parameter = ListTablesRequest;
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
        Some(input_schema::<Self::Parameter>(true))
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

impl AsyncTool<SqliteHandler> for ListTablesTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_tables(params).await
    }
}

impl SqliteHandler {
    /// Lists one page of tables in the connected database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest {
            cursor,
            search,
            detailed,
        }: ListTablesRequest,
    ) -> Result<ListEntriesResponse, ErrorData> {
        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::tables::names(&self.connection, page, pattern).await,
            async |page, pattern| queries::tables::details(&self.connection, page, pattern).await,
        )
        .await
        .map_err(Into::into)
    }
}
