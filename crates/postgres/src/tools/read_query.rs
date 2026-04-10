//! MCP tool: `read_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{QueryRequest, QueryResponse};
use database_mcp_sql::timeout::execute_with_timeout;
use database_mcp_sql::validation::validate_read_only_with_dialect;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx_to_json::RowExt;

use crate::PostgresHandler;

/// Marker type for the `read_query` MCP tool.
pub(crate) struct ReadQueryTool;

impl ReadQueryTool {
    const NAME: &'static str = "read_query";
    const DESCRIPTION: &'static str = "Execute a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).";
}

impl ToolBase for ReadQueryTool {
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
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::PostgreSqlDialect {})?;
        let db = Some(request.database_name.trim()).filter(|s| !s.is_empty());
        let pool = self.get_pool(db).await?;
        let rows: Vec<PgRow> = execute_with_timeout(
            self.config.query_timeout,
            &request.query,
            sqlx::query(&request.query).fetch_all(&pool),
        )
        .await?;
        Ok(QueryResponse {
            rows: Value::Array(rows.iter().map(RowExt::to_json).collect()),
        })
    }
}
