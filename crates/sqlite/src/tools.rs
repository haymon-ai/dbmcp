//! MCP tool definitions for the `SQLite` backend.
//!
//! Uses rmcp `#[tool]` attribute macros to define tools as methods
//! on [`SqliteAdapter`], eliminating manual [`ToolBase`] and
//! [`AsyncTool`] implementations.

use database_mcp_server::map_error;
use database_mcp_server::types::{GetTableSchemaRequest, ListTablesRequest, QueryRequest};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;

use super::SqliteAdapter;

impl SqliteAdapter {
    /// Names of tools that require write access.
    const WRITE_TOOL_NAMES: &[&str] = &["write_query"];

    /// Builds the tool router, excluding write tools in read-only mode.
    #[must_use]
    pub fn build_tool_router(&self) -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        if self.config.read_only {
            for name in Self::WRITE_TOOL_NAMES {
                router.remove_route(name);
            }
        }
        router
    }
}

#[rmcp::tool_router]
impl SqliteAdapter {
    /// List all tables in a specific database.
    /// Requires `database_name` from `list_databases`.
    #[tool(
        name = "list_tables",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn tool_list_tables(&self, params: Parameters<ListTablesRequest>) -> Result<CallToolResult, ErrorData> {
        let req = params.0;
        let result = self.list_tables(&req.database_name).await.map_err(map_error)?;
        let json = serde_json::to_string_pretty(&result).map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get column definitions (type, nullable, key, default) and foreign key
    /// relationships for a table. Requires `database_name` and `table_name`.
    #[tool(
        name = "get_table_schema",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn tool_get_table_schema(
        &self,
        params: Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let req = params.0;
        let result = self
            .get_table_schema(&req.database_name, &req.table_name)
            .await
            .map_err(map_error)?;
        let json = serde_json::to_string_pretty(&result).map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Execute a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
    #[tool(
        name = "read_query",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn tool_read_query(&self, params: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        let req = params.0;
        database_mcp_sql::validation::validate_read_only_with_dialect(
            &req.sql_query,
            &sqlparser::dialect::SQLiteDialect {},
        )
        .map_err(map_error)?;

        let db = if req.database_name.is_empty() {
            None
        } else {
            Some(req.database_name.as_str())
        };
        let result = self.execute_query(&req.sql_query, db).await.map_err(map_error)?;
        let json = serde_json::to_string_pretty(&result).map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
    #[tool(
        name = "write_query",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn tool_write_query(&self, params: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        let req = params.0;
        let db = if req.database_name.is_empty() {
            None
        } else {
            Some(req.database_name.as_str())
        };
        let result = self.execute_query(&req.sql_query, db).await.map_err(map_error)?;
        let json = serde_json::to_string_pretty(&result).map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
