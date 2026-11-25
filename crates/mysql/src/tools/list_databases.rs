//! MCP tool: `listDatabases`.

use dbmcp_server::listing;
use dbmcp_server::types::{ListDatabasesRequest, ListDatabasesResponse};

use super::prelude::*;
use crate::queries;

const NAME: &str = "listDatabases";
const TITLE: &str = "List Databases";
const DESCRIPTION: &str = include_str!("../../assets/tools/list_databases/default.md");

/// Marker type for the `listDatabases` MCP tool.
pub(crate) struct ListDatabasesTool;

impl ToolBase for ListDatabasesTool {
    type Parameter = ListDatabasesRequest;
    type Output = ListDatabasesResponse;
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
        Some(input_schema::<Self::Parameter>(false))
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

impl AsyncTool<MysqlHandler> for ListDatabasesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_databases(params).await
    }
}

impl MysqlHandler {
    /// Lists one page of accessible databases.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_databases(
        &self,
        ListDatabasesRequest { cursor }: ListDatabasesRequest,
    ) -> Result<ListDatabasesResponse, ErrorData> {
        let (databases, next_cursor) = listing::paginate(cursor, self.config.page_size, async |page| {
            queries::databases::names(&self.connection, page).await
        })
        .await?;

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}
