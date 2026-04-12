//! MCP tool: `list_tables`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{ListTablesRequest, ListTablesResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `list_tables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "list_tables";
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

impl AsyncTool<MysqlHandler> for ListTablesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_tables(&params).await?)
    }
}

impl MysqlHandler {
    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse, AppError> {
        validate_identifier(&request.database_name)?;
        let sql = format!(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            self.connection.quote_string(&request.database_name)
        );
        let rows = self.connection.fetch(sql.as_str(), None).await?;
        Ok(ListTablesResponse {
            tables: rows
                .iter()
                .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
                .collect(),
        })
    }
}
