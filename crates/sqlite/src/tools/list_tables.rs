//! MCP tool: `list_tables`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::ListTablesResponse;

use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ListTablesRequest;

/// Marker type for the `list_tables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "list_tables";
    const TITLE: &'static str = "List Tables";
    const DESCRIPTION: &'static str = r#"List all tables in the connected SQLite database. Use this tool to discover what tables are available before using other tools.

<usecase>
ALWAYS call this tool FIRST when:
- You need to explore what tables exist in the database
- You need a table name for get_table_schema or query tools
- The user asks what data is available
</usecase>

<examples>
✓ "What tables are in this database?"
✓ "Does a users table exist?" → list_tables to check
✗ "Show me the columns of users" → use get_table_schema instead
</examples>

<what_it_returns>
A sorted JSON array of table name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>"#;
}

impl ToolBase for ListTablesTool {
    type Parameter = ListTablesRequest;
    type Output = ListTablesResponse;
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        None
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
    /// Lists one page of tables in the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest { cursor }: ListTablesRequest,
    ) -> Result<ListTablesResponse, ErrorData> {
        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT name
            FROM sqlite_master
            WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (tables, next_cursor) = pager.finalize(rows);

        Ok(ListTablesResponse { tables, next_cursor })
    }
}
