//! MCP tool: `listDatabases`.

use dbmcp_server::listing;
use dbmcp_server::types::{ListDatabasesRequest, ListDatabasesResponse};

use super::prelude::*;

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

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(false))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for ListDatabasesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_databases(params).await
    }
}

/// Lists one page of schema names, sorted by name.
const SQL: &str = r"
    SELECT CAST(SCHEMA_NAME AS CHAR)
    FROM information_schema.SCHEMATA
    ORDER BY SCHEMA_NAME
    LIMIT ? OFFSET ?";

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
            self.connection
                .fetch_scalar(sqlx::query(SQL).bind(page.limit()).bind(page.offset()), None)
                .await
        })
        .await?;

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}
