//! MCP tool: `listDatabases`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListDatabasesRequest, ListDatabasesResponse};
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `listDatabases` MCP tool.
pub(crate) struct ListDatabasesTool;

impl ListDatabasesTool {
    const NAME: &'static str = "listDatabases";
    const TITLE: &'static str = "List Databases";
    const DESCRIPTION: &'static str = r#"List all accessible databases on the connected server. Use this tool to discover what databases are available before using other tools.

<usecase>
ALWAYS call this tool FIRST when:
- You need to explore what databases exist on the server
- You need a database name for listTables, getTableSchema, or query tools
- The user asks what data is available
</usecase>

<examples>
✓ "What databases are on this server?"
✓ "Show me what's available" → call listDatabases first
</examples>

<what_it_returns>
A sorted JSON array of database name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>"#;
}

impl ToolBase for ListDatabasesTool {
    type Parameter = ListDatabasesRequest;
    type Output = ListDatabasesResponse;
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
        let (databases, next_cursor) = pager.finalize(rows);

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}
