//! MCP tool: `listTriggers`.

use dbmcp_server::listing;
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::queries;
use crate::types::ListTriggersRequest;

const NAME: &str = "listTriggers";
const TITLE: &str = "List Triggers";
const DESCRIPTION: &str = include_str!("../../assets/tools/list_triggers.md");

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ToolBase for ListTriggersTool {
    type Parameter = ListTriggersRequest;
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

impl AsyncTool<SqliteHandler> for ListTriggersTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_triggers(params).await
    }
}

impl SqliteHandler {
    /// Lists one page of triggers in the connected database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest {
            cursor,
            search,
            detailed,
        }: ListTriggersRequest,
    ) -> Result<ListEntriesResponse, ErrorData> {
        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::triggers::names(&self.connection, page, pattern).await,
            async |page, pattern| queries::triggers::details(&self.connection, page, pattern).await,
        )
        .await
        .map_err(Into::into)
    }
}
