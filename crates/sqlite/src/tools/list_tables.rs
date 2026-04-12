//! MCP tool: `list_tables`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::AppError;
use database_mcp_server::types::ListTablesResponse;
use database_mcp_sql::Connection as _;
use rmcp::handler::server::common::schema_for_empty_input;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};
use serde_json::Value;

use crate::SqliteHandler;

/// Marker type for the `list_tables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "list_tables";
    const DESCRIPTION: &'static str = "List all tables in the connected `SQLite` database.";
}

impl ToolBase for ListTablesTool {
    type Parameter = ();
    type Output = ListTablesResponse;
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

impl AsyncTool<SqliteHandler> for ListTablesTool {
    async fn invoke(handler: &SqliteHandler, _params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_tables().await?)
    }
}

impl SqliteHandler {
    /// Lists all tables in the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_tables(&self) -> Result<ListTablesResponse, AppError> {
        let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name";
        let rows = self.connection.fetch(sql, None).await?;
        let tables = rows
            .iter()
            .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        Ok(ListTablesResponse { tables })
    }
}
