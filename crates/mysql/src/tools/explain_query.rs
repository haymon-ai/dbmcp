//! MCP tool: `explain_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{ExplainQueryRequest, QueryResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::validation::validate_read_only_with_dialect;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::MysqlHandler;

/// Marker type for the `explain_query` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ExplainQueryTool {
    const NAME: &'static str = "explain_query";
    const DESCRIPTION: &'static str = r#"Return the execution plan for a SQL query to diagnose performance. Use this tool instead of running EXPLAIN directly through read_query — it provides structured output.

<usecase>
Use when:
- A query runs slowly and you need to understand why
- Investigating performance bottlenecks
- Planning index creation to optimize queries
- Analyzing join methods, table scan strategies, and sort operations
</usecase>

<when_not_to_use>
- Running actual queries → use read_query or write_query
- Checking table structure → use get_table_schema
</when_not_to_use>

<examples>
✓ "Why is my SELECT on orders slow?" → explain_query(query="SELECT ...")
✓ "Should I add an index?" → explain_query with analyze=true
✗ "Run this SELECT" → use read_query
</examples>

<safety>
Set `analyze` to true for actual execution statistics (EXPLAIN ANALYZE).
IMPORTANT: EXPLAIN ANALYZE actually executes the query! In read-only mode, only read-only statements are allowed with analyze.
When analyze is false, returns EXPLAIN FORMAT=JSON output without executing.
</safety>

<what_it_returns>
A JSON array of execution plan rows showing access methods, join types, row estimates, and costs.
</what_it_returns>"#;
}

impl ToolBase for ExplainQueryTool {
    type Parameter = ExplainQueryRequest;
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
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }
}

impl AsyncTool<MysqlHandler> for ExplainQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.explain_query(&params).await?)
    }
}

impl MysqlHandler {
    /// Returns the execution plan for a query.
    ///
    /// When `analyze` is true and read-only mode is enabled, the inner
    /// query is validated to be read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if `analyze` is true,
    /// read-only mode is enabled, and the query is a write statement.
    /// Returns [`AppError::Query`] if the backend reports an error.
    pub async fn explain_query(&self, request: &ExplainQueryRequest) -> Result<QueryResponse, AppError> {
        if request.analyze && self.config.read_only {
            validate_read_only_with_dialect(&request.query, &sqlparser::dialect::MySqlDialect {})?;
        }

        let explain_sql = if request.analyze {
            format!("EXPLAIN ANALYZE {}", request.query)
        } else {
            format!("EXPLAIN FORMAT=JSON {}", request.query)
        };

        let rows = self
            .connection
            .fetch(explain_sql.as_str(), Some(request.database_name.as_str()))
            .await?;
        Ok(QueryResponse {
            rows: Value::Array(rows),
        })
    }
}
