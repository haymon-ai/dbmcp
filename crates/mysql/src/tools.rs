//! MCP tool definitions for the MySQL/MariaDB backend.
//!
//! Uses rmcp `#[tool]` attribute macros to define tools as methods
//! on [`MysqlAdapter`], eliminating manual [`ToolBase`] and
//! [`AsyncTool`] implementations.

use super::types::DropTableRequest;
use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListDatabasesResponse,
    ListTablesRequest, ListTablesResponse, MessageResponse, QueryRequest, QueryResponse, TableSchemaResponse,
};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::ErrorData;
use rmcp::tool;

use super::MysqlAdapter;

impl MysqlAdapter {
    /// Names of tools that require write access.
    const WRITE_TOOL_NAMES: &[&str] = &["write_query", "create_database", "drop_database", "drop_table"];

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
impl MysqlAdapter {
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
    pub async fn tool_list_databases(&self) -> Result<Json<ListDatabasesResponse>, ErrorData> {
        Ok(Json(self.list_databases().await?))
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
    ) -> Result<Json<MessageResponse>, ErrorData> {
        Ok(Json(self.create_database(&request).await?))
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
    ) -> Result<Json<MessageResponse>, ErrorData> {
        Ok(Json(self.drop_database(&request).await?))
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
    ) -> Result<Json<ListTablesResponse>, ErrorData> {
        Ok(Json(self.list_tables(&request).await?))
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
    ) -> Result<Json<TableSchemaResponse>, ErrorData> {
        Ok(Json(self.get_table_schema(&request).await?))
    }

    /// Drop a table from a database.
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
    ) -> Result<Json<MessageResponse>, ErrorData> {
        Ok(Json(self.drop_table(&request).await?))
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
    ) -> Result<Json<QueryResponse>, ErrorData> {
        Ok(Json(self.read_query(&request).await?))
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
    ) -> Result<Json<QueryResponse>, ErrorData> {
        Ok(Json(self.write_query(&request).await?))
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
    ) -> Result<Json<QueryResponse>, ErrorData> {
        Ok(Json(self.explain_query(&request).await?))
    }
}
