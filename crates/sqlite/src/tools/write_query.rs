//! MCP tool: `write_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::QueryResponse;
use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::SqliteHandler;
use crate::types::QueryRequest;

/// Marker type for the `write_query` MCP tool.
pub(crate) struct WriteQueryTool;

impl WriteQueryTool {
    const NAME: &'static str = "write_query";
    const DESCRIPTION: &'static str = r#"Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

<usecase>
Use when:
- Inserting, updating, or deleting rows
- Creating or altering tables, indexes, views, or other schema objects
- Any data modification operation
</usecase>

<when_not_to_use>
- Read-only queries (SELECT) → use read_query
- Query performance analysis → use explain_query
</when_not_to_use>

<examples>
✓ "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
✓ "UPDATE orders SET status = 'shipped' WHERE id = 42"
✓ "CREATE TABLE logs (id INTEGER PRIMARY KEY, message TEXT)"
✗ "SELECT * FROM users" → use read_query
</examples>

<what_it_returns>
A JSON array of affected/returning row objects, each keyed by column name.
</what_it_returns>"#;
}

impl ToolBase for WriteQueryTool {
    type Parameter = QueryRequest;
    type Output = QueryResponse;
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
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        )
    }
}

impl AsyncTool<SqliteHandler> for WriteQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.write_query(&params).await?)
    }
}

impl SqliteHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn write_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        let rows = self.connection.fetch(request.query.as_str(), None).await?;
        Ok(QueryResponse {
            rows: Value::Array(rows),
        })
    }
}
