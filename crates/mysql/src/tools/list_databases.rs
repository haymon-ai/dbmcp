//! MCP tool: `listDatabases`.

use dbmcp_server::pagination::Pager;
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

impl MysqlHandler {
    /// Lists one page of accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_databases(
        &self,
        ListDatabasesRequest { cursor }: ListDatabasesRequest,
    ) -> Result<ListDatabasesResponse, ErrorData> {
        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT CAST(SCHEMA_NAME AS CHAR)
            FROM information_schema.SCHEMATA
            ORDER BY SCHEMA_NAME
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (databases, next_cursor) = pager.paginate(rows);

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}
