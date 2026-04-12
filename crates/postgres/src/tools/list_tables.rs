//! MCP tool: `list_tables`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{ListTablesRequest, ListTablesResponse};
use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::PostgresHandler;

/// Marker type for the `list_tables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "list_tables";
    const DESCRIPTION: &'static str =
        "List all tables in a specific database.\nRequires `database_name` from `list_databases`.";
}

impl ToolBase for ListTablesTool {
    type Parameter = ListTablesRequest;
    type Output = ListTablesResponse;
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
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for ListTablesTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_tables(&params).await?)
    }
}

impl PostgresHandler {
    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse, AppError> {
        let db = if request.database_name.is_empty() {
            None
        } else {
            Some(request.database_name.as_str())
        };
        let sql = "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename";
        let rows = self.connection.fetch(sql, db).await?;
        Ok(ListTablesResponse {
            tables: rows
                .iter()
                .filter_map(|r| r.get("tablename").and_then(Value::as_str).map(str::to_owned))
                .collect(),
        })
    }
}
