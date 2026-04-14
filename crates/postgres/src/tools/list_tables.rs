//! MCP tool: `list_tables`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{ListTablesRequest, ListTablesResponse};
use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::PostgresHandler;

/// Marker type for the `list_tables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "list_tables";
    const TITLE: &'static str = "List Tables";
    const DESCRIPTION: &'static str = r#"List all tables in a specific database. Requires `database_name` — call `list_databases` first to discover available databases.

<usecase>
Use when:
- Exploring a database to find relevant tables
- Verifying a table exists before querying or inspecting it
- The user asks what tables are in a database
</usecase>

<examples>
✓ "What tables are in the mydb database?" → list_tables(database_name="mydb")
✓ "Does a users table exist?" → list_tables to check
✗ "Show me the columns of users" → use get_table_schema instead
</examples>

<what_it_returns>
A sorted JSON array of table name strings.
</what_it_returns>"#;
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

impl AsyncTool<PostgresHandler> for ListTablesTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_tables(&params).await?)
    }
}

impl PostgresHandler {
    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse, AppError> {
        let db = if request.database_name.is_empty() {
            None
        } else {
            Some(request.database_name.as_str())
        };
        let sql = "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename";
        let rows = self.connection.fetch(sql, db).await?;
        Ok(ListTablesResponse {
            tables: rows
                .iter()
                .filter_map(|r| r.get("tablename").and_then(Value::as_str).map(str::to_owned))
                .collect(),
        })
    }
}
