//! MCP tool: `explainQuery`.

use std::borrow::Cow;

use dbmcp_server::types::{ExplainQueryRequest, QueryResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use dbmcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `explainQuery` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ExplainQueryTool {
    const NAME: &'static str = "explainQuery";
    const TITLE: &'static str = "Explain Query";
    const DESCRIPTION: &'static str = r#"Return the execution plan for a SQL query to diagnose performance. Use this tool instead of running EXPLAIN directly through readQuery — it provides structured JSON output.

<usecase>
Use when:
- A query runs slowly and you need to understand why
- Investigating performance bottlenecks
- Planning index creation to optimize queries
- Analyzing join methods, table scan strategies, and sort operations
</usecase>

<when_not_to_use>
- Running actual queries → use readQuery or writeQuery
- Checking table structure → use listTables(detailed=true)
</when_not_to_use>

<examples>
✓ "Why is my SELECT on orders slow?" → explainQuery(query="SELECT ...")
✓ "Should I add an index?" → explainQuery with analyze=true
✗ "Run this SELECT" → use readQuery
</examples>

<safety>
Set `analyze` to true for actual execution statistics (EXPLAIN ANALYZE).
IMPORTANT: EXPLAIN ANALYZE actually executes the query! In read-only mode, only read-only statements are allowed with analyze.
When analyze is false, returns EXPLAIN (FORMAT JSON) output without executing.
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

impl AsyncTool<PostgresHandler> for ExplainQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.explain_query(params).await?)
    }
}

impl PostgresHandler {
    /// Returns the execution plan for a query.
    ///
    /// When `analyze` is true and read-only mode is enabled, the inner
    /// query is validated to be read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if `analyze` is true,
    /// read-only mode is enabled, and the query is a write statement.
    /// Returns [`SqlError::Query`] if the backend reports an error.
    pub async fn explain_query(
        &self,
        ExplainQueryRequest {
            database,
            query,
            analyze,
        }: ExplainQueryRequest,
    ) -> Result<QueryResponse, SqlError> {
        if analyze && self.config.read_only {
            let _ = validate_read_only(&query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = if analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}")
        } else {
            format!("EXPLAIN (FORMAT JSON) {query}")
        };

        let rows = self.connection.fetch_json(explain_sql.as_str(), database).await?;

        Ok(QueryResponse { rows })
    }
}
