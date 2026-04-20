//! MCP tool: `writeQuery`.

use std::borrow::Cow;

use database_mcp_server::types::{QueryRequest, QueryResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::SqlError;
use database_mcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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
- Read-only queries (SELECT, SHOW) → use readQuery
- Query performance analysis → use explainQuery
- Creating/dropping entire databases → use createDatabase or dropDatabase
</when_not_to_use>

<examples>
✓ "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
✓ "UPDATE orders SET status = 'shipped' WHERE id = 42"
✓ "CREATE TABLE logs (id INT PRIMARY KEY, message TEXT)"
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

impl AsyncTool<MysqlHandler> for WriteQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.write_query(params).await?)
    }
}

impl MysqlHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, QueryRequest { query, database }: QueryRequest) -> Result<QueryResponse, SqlError> {
        let db = Some(database.trim()).filter(|s| !s.is_empty());

        if let Some(name) = &db {
            validate_ident(name)?;
        }

        let rows = self.connection.fetch_json(query.as_str(), db).await?;

        Ok(QueryResponse { rows })
    }
}
