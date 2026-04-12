//! MCP tool: `list_databases`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::AppError;
use database_mcp_server::types::ListDatabasesResponse;
use database_mcp_sql::Connection as _;
use rmcp::handler::server::common::schema_for_empty_input;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};
use serde_json::Value;

use crate::PostgresHandler;

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
        Some(schema_for_empty_input())
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

impl AsyncTool<PostgresHandler> for ListDatabasesTool {
    async fn invoke(handler: &PostgresHandler, _params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_databases().await?)
    }
}

impl PostgresHandler {
    /// Lists all accessible databases.
    ///
    /// Uses the default pool intentionally — `pg_database` is a server-wide
    /// catalog that returns all databases regardless of which database the
    /// connection targets.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_databases(&self) -> Result<ListDatabasesResponse, AppError> {
        let sql = "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname";
        let rows = self.connection.fetch(sql, None).await?;
        Ok(ListDatabasesResponse {
            databases: rows
                .iter()
                .filter_map(|r| r.get("datname").and_then(Value::as_str).map(str::to_owned))
                .collect(),
        })
    }
}
