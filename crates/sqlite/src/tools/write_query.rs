//! MCP tool: `writeQuery`.

use std::borrow::Cow;

use dbmcp_server::types::QueryResponse;

use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::QueryRequest;

/// Marker type for the `writeQuery` MCP tool.
pub(crate) struct WriteQueryTool;

impl WriteQueryTool {
    const NAME: &'static str = "writeQuery";
    const TITLE: &'static str = "Write Query";
    const DESCRIPTION: &'static str = r#"Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

<usecase>
Use when:
- Inserting, updating, or deleting rows
- Creating or altering tables, indexes, views, or other schema objects
- Any data modification operation
</usecase>

<when_not_to_use>
- Read-only queries (SELECT) → use readQuery
- Query performance analysis → use explainQuery
</when_not_to_use>

<examples>
✓ "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
✓ "UPDATE orders SET status = 'shipped' WHERE id = 42"
✓ "CREATE TABLE logs (id INTEGER PRIMARY KEY, message TEXT)"
✗ "SELECT * FROM users" → use readQuery
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

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
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
        Ok(handler.write_query(params).await?)
    }
}

impl SqliteHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, QueryRequest { query }: QueryRequest) -> Result<QueryResponse, SqlError> {
        let rows = self.connection.fetch_json(query.as_str(), None).await?;
        Ok(QueryResponse { rows })
    }
}
