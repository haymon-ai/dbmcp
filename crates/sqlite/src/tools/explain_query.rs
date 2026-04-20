//! MCP tool: `explain_query`.

use std::borrow::Cow;

use database_mcp_server::types::QueryResponse;

use database_mcp_sql::Connection as _;
use database_mcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ExplainQueryRequest;

/// Marker type for the `explain_query` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ExplainQueryTool {
    const NAME: &'static str = "explain_query";
    const TITLE: &'static str = "Explain Query";
    const DESCRIPTION: &'static str = r#"Return the execution plan for a SQL query to diagnose performance. Use this tool instead of running EXPLAIN directly through read_query — it provides structured output via EXPLAIN QUERY PLAN.

<usecase>
Use when:
- A query runs slowly and you need to understand why
- Understanding how SQLite will scan tables and use indexes
- Deciding whether to add an index
</usecase>

<when_not_to_use>
- Running actual queries → use read_query or write_query
- Checking table structure → use get_table_schema
</when_not_to_use>

<examples>
✓ "Why is my SELECT on orders slow?" → explain_query(query="SELECT ...")
✓ "How will SQLite execute this join?" → explain_query
✗ "Run this SELECT" → use read_query
</examples>

<what_it_returns>
A JSON array of EXPLAIN QUERY PLAN rows showing how SQLite will scan tables, use indexes, and order operations.
</what_it_returns>"#;
}

impl ToolBase for ExplainQueryTool {
    type Parameter = ExplainQueryRequest;
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

impl AsyncTool<SqliteHandler> for ExplainQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.explain_query(params).await?)
    }
}

impl SqliteHandler {
    /// Returns the execution plan for a query.
    ///
    /// Always uses `EXPLAIN QUERY PLAN` — `SQLite` does not support
    /// `EXPLAIN ANALYZE`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::Query`] if the backend reports an error.
    pub async fn explain_query(
        &self,
        ExplainQueryRequest { query }: ExplainQueryRequest,
    ) -> Result<QueryResponse, SqlError> {
        let explain_sql = format!("EXPLAIN QUERY PLAN {query}");

        let rows = self.connection.fetch_json(explain_sql.as_str(), None).await?;

        Ok(QueryResponse { rows })
    }
}
