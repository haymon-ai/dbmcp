//! MCP tool: `explainQuery`.

use std::borrow::Cow;

use dbmcp_server::types::{ExplainQueryRequest, QueryResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `explainQuery` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ExplainQueryTool {
    const NAME: &'static str = "explainQuery";
    const TITLE: &'static str = "Explain Query";
    const DESCRIPTION: &'static str = include_str!("../../assets/tools/explain_query.md");
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
        handler.explain_query(params).await
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
    ) -> Result<QueryResponse, ErrorData> {
        if analyze && self.config.read_only {
            let _ = validate_read_only(&query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = if analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}")
        } else {
            format!("EXPLAIN (FORMAT JSON) {query}")
        };

        let mut rows = self.connection.fetch_json(explain_sql.as_str(), database).await?;
        if let Some(r) = &self.redactor {
            r.apply(&mut rows)?;
        }

        Ok(QueryResponse { rows })
    }
}
