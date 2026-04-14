//! MCP tool: `list_databases`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::AppError;
use database_mcp_server::types::ListDatabasesResponse;
use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `list_databases` MCP tool.
pub(crate) struct ListDatabasesTool;

impl ListDatabasesTool {
    const NAME: &'static str = "list_databases";
    const DESCRIPTION: &'static str = r#"List all accessible databases on the connected server. Use this tool to discover what databases are available before using other tools.

<usecase>
ALWAYS call this tool FIRST when:
- You need to explore what databases exist on the server
- You need a database name for list_tables, get_table_schema, or query tools
- The user asks what data is available
</usecase>

<examples>
✓ "What databases are on this server?"
✓ "Show me what's available" → call list_databases first
</examples>

<what_it_returns>
A sorted JSON array of database name strings.
</what_it_returns>"#;
}

impl ToolBase for ListDatabasesTool {
    type Parameter = ();
    type Output = ListDatabasesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
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

impl AsyncTool<MysqlHandler> for ListDatabasesTool {
    async fn invoke(handler: &MysqlHandler, _params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_databases().await?)
    }
}

impl MysqlHandler {
    /// Lists all accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_databases(&self) -> Result<ListDatabasesResponse, AppError> {
        let sql = "SELECT SCHEMA_NAME AS name FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME";
        let rows = self.connection.fetch(sql, None).await?;
        Ok(ListDatabasesResponse {
            databases: rows
                .iter()
                .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
                .collect(),
        })
    }
}
