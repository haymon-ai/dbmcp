//! MCP tool: `listTriggers`.

use dbmcp_server::listing::{self, Cursor};
use dbmcp_server::types::{ListEntriesResponse, ListTriggersRequest};

use super::prelude::*;
use crate::queries;

const NAME: &str = "listTriggers";
const TITLE: &str = "List Triggers";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/list_triggers/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/list_triggers/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ListTriggersTool<PINNED> {
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

impl<const PINNED: bool> AsyncTool<MysqlHandler> for ListTriggersTool<PINNED> {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_triggers(params.database, params.cursor, params.search, params.detailed)
            .await
    }
}

impl MysqlHandler {
    /// Lists one page of user-defined triggers, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_triggers(
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
            async |page, pattern| queries::triggers::names(&self.connection, page, pattern, database).await,
            async |page, pattern| queries::triggers::details(&self.connection, page, pattern, database).await,
        )
        .await
        .map_err(Into::into)
    }
}
