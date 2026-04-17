//! MCP tool: `read_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{QueryRequest, QueryResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::validate_ident;
use database_mcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::PostgresHandler;

/// Marker type for the `read_query` MCP tool.
pub(crate) struct ReadQueryTool;

impl ReadQueryTool {
    const NAME: &'static str = "read_query";
    const TITLE: &'static str = "Read Query";
    const DESCRIPTION: &'static str = r#"Execute a read-only SQL query. Allowed statements: SELECT, SHOW, EXPLAIN. Accepts an optional `database_name` to query across databases without reconnecting.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Listing server configuration parameters (SHOW)
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use write_query
- Query performance analysis → use explain_query
- Discovering tables or columns → use list_tables or get_table_schema
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✓ "SHOW server_version"
✗ "INSERT INTO users ..." → use write_query
✗ "EXPLAIN SELECT ..." → use explain_query for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.
</what_it_returns>"#;
}

impl ToolBase for ReadQueryTool {
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
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }
}

impl AsyncTool<PostgresHandler> for ReadQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.read_query(&params).await?)
    }
}

impl PostgresHandler {
    /// Executes a read-only SQL query.
    ///
    /// Validates that the query is read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`AppError::Query`] if the backend reports an error.
    pub async fn read_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        let QueryRequest { query, database_name } = request;

        validate_read_only(query, &sqlparser::dialect::PostgreSqlDialect {})?;
        let db = Some(database_name.trim()).filter(|s| !s.is_empty());
        if let Some(name) = &db {
            validate_ident(name)?;
        }
        let rows = self.connection.fetch_json(query.as_str(), db).await?;
        Ok(QueryResponse {
            rows: Value::Array(rows),
        })
    }
}
