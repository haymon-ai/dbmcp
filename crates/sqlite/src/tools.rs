//! MCP tool definitions for the `SQLite` backend.
//!
//! Uses rmcp `#[tool]` attribute macros to define tools as methods
//! on [`SqliteAdapter`], eliminating manual [`ToolBase`] and
//! [`AsyncTool`] implementations.

use super::types::{DropTableRequest, ExplainQueryRequest, GetTableSchemaRequest, QueryRequest};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;

use database_mcp_sql::validation::validate_read_only_with_dialect;

use super::SqliteAdapter;

impl SqliteAdapter {
    /// Names of tools that require write access.
    const WRITE_TOOL_NAMES: &[&str] = &["write_query", "drop_table"];

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
    /// List all tables in the connected `SQLite` database.
    #[tool(
        name = "list_tables",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn tool_list_tables(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.list_tables().await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// Get column definitions (type, nullable, key, default) and foreign key
    /// relationships for a table.
    #[tool(
        name = "get_table_schema",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn tool_get_table_schema(
        &self,
        Parameters(request): Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.get_table_schema(&request.table_name).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
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
    pub async fn tool_read_query(
        &self,
        Parameters(request): Parameters<QueryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::SQLiteDialect {})?;

        let result = self.execute_query(&request.query).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// Return the execution plan for a SQL query.
    #[tool(
        name = "explain_query",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn tool_explain_query(
        &self,
        Parameters(request): Parameters<ExplainQueryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.explain_query(&request.query).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// Drop a table from the database.
    #[tool(
        name = "drop_table",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn tool_drop_table(
        &self,
        Parameters(request): Parameters<DropTableRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.drop_table(&request.table_name).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
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
    pub async fn tool_write_query(
        &self,
        Parameters(request): Parameters<QueryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.execute_query(&request.query).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }
}
