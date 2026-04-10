//! MCP tool: `explain_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::QueryResponse;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx_to_json::RowExt;

use crate::SqliteHandler;
use crate::types::ExplainQueryRequest;

/// Marker type for the `explain_query` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ExplainQueryTool {
    const NAME: &'static str = "explain_query";
    const DESCRIPTION: &'static str = "Return the execution plan for a SQL query.";
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

impl AsyncTool<SqliteHandler> for ExplainQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.explain_query(&params).await?)
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
    /// Returns [`AppError::Query`] if the backend reports an error.
    pub async fn explain_query(&self, request: &ExplainQueryRequest) -> Result<QueryResponse, AppError> {
        let pool = self.pool.clone();
        let explain_sql = format!("EXPLAIN QUERY PLAN {}", request.query);
        let rows: Vec<SqliteRow> = execute_with_timeout(
            self.config.query_timeout,
            &explain_sql,
            sqlx::query(&explain_sql).fetch_all(&pool),
        )
        .await?;
        Ok(QueryResponse {
            rows: Value::Array(rows.iter().map(RowExt::to_json).collect()),
        })
    }
}
