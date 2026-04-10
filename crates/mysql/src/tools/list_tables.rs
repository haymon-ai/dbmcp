//! MCP tool: `list_tables`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{ListTablesRequest, ListTablesResponse};
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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

impl AsyncTool<MysqlHandler> for ListTablesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.list_tables(&params).await?)
    }
}

impl MysqlHandler {
    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse, AppError> {
        validate_identifier(&request.database_name)?;
        let sql = format!(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            Self::quote_string(&request.database_name)
        );
        let results = self.query_to_json(&sql, None).await?;
        let rows = results.as_array().map_or([].as_slice(), Vec::as_slice);
        Ok(ListTablesResponse {
            tables: rows
                .iter()
                .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
                .collect(),
        })
    }
}
