//! MCP tool definitions for the `SQLite` backend.
//!
//! Uses rmcp `#[tool]` attribute macros to define tools as methods
//! on [`SqliteAdapter`], eliminating manual [`ToolBase`] and
//! [`AsyncTool`] implementations.

use super::types::{DropTableRequest, ExplainQueryRequest, GetTableSchemaRequest, QueryRequest};
use database_mcp_server::types::{ListTablesResponse, MessageResponse, QueryResponse, TableSchemaResponse};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::ErrorData;
use rmcp::tool;

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
    pub async fn tool_list_tables(&self) -> Result<Json<ListTablesResponse>, ErrorData> {
        Ok(Json(self.list_tables().await?))
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
    ) -> Result<Json<TableSchemaResponse>, ErrorData> {
        Ok(Json(self.get_table_schema(&request).await?))
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
