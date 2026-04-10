//! MCP tool: `write_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{QueryRequest, QueryResponse};
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx_to_json::RowExt;

use crate::PostgresHandler;

/// Marker type for the `write_query` MCP tool.
pub(crate) struct WriteQueryTool;

impl WriteQueryTool {
    const NAME: &'static str = "write_query";
    const DESCRIPTION: &'static str = "Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).";
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

impl AsyncTool<PostgresHandler> for WriteQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.write_query(&params).await?)
    }
}

impl PostgresHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn write_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
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
