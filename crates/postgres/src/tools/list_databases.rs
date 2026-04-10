//! MCP tool: `list_databases`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::AppError;
use database_mcp_server::types::ListDatabasesResponse;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::common::schema_for_empty_input;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `list_databases` MCP tool.
pub(crate) struct ListDatabasesTool;

impl ListDatabasesTool {
    const NAME: &'static str = "list_databases";
    const DESCRIPTION: &'static str = "List all accessible databases on the connected database server.\nCall this first to discover available database names.";
}

impl ToolBase for ListDatabasesTool {
    type Parameter = ();
    type Output = ListDatabasesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(schema_for_empty_input())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for ListDatabasesTool {
    async fn invoke(handler: &PostgresHandler, _params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_databases().await?)
    }
}

impl PostgresHandler {
    /// Lists all accessible databases.
    ///
    /// Uses the default pool intentionally — `pg_database` is a server-wide
    /// catalog that returns all databases regardless of which database the
    /// connection targets.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_databases(&self) -> Result<ListDatabasesResponse, AppError> {
        let pool = self.get_pool(None).await?;
        let sql = "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(ListDatabasesResponse {
            databases: rows.into_iter().map(|r| r.0).collect(),
        })
    }
}
