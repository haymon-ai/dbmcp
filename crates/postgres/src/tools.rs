//! MCP tool definitions for the `PostgreSQL` backend.
//!
//! Uses rmcp `#[tool]` attribute macros to define tools as methods
//! on [`PostgresAdapter`], eliminating manual [`ToolBase`] and
//! [`AsyncTool`] implementations.

use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest,
};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;

use database_mcp_sql::validation::validate_read_only_with_dialect;

use super::PostgresAdapter;

impl PostgresAdapter {
    /// Names of tools that require write access.
    const WRITE_TOOL_NAMES: &[&str] = &["write_query", "create_database", "drop_database"];

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
impl PostgresAdapter {
    /// List all accessible databases on the connected database server.
    /// Call this first to discover available database names.
    #[tool(
        name = "list_databases",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn tool_list_databases(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.list_databases().await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

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
    pub async fn tool_list_tables(
        &self,
        Parameters(request): Parameters<ListTablesRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.list_tables(&request.database_name).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
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
    pub async fn tool_get_table_schema(
        &self,
        Parameters(request): Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .get_table_schema(&request.database_name, &request.table_name)
            .await?;
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
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::PostgreSqlDialect {})?;

        let db = Some(request.database_name.trim()).filter(|s| !s.is_empty());
        let result = self.execute_query(&request.query, db).await?;
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
        let db = Some(request.database_name.trim()).filter(|s| !s.is_empty());
        let result = self.execute_query(&request.query, db).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// Create a new database.
    #[tool(
        name = "create_database",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn tool_create_database(
        &self,
        Parameters(request): Parameters<CreateDatabaseRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.create_database(&request.database_name).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// Drop an existing database. Cannot drop the currently connected database.
    #[tool(
        name = "drop_database",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn tool_drop_database(
        &self,
        Parameters(request): Parameters<DropDatabaseRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.drop_database(&request.database_name).await?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }
}
