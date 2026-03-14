//! MCP server setup, tool registration, and transport dispatch.
//!
//! Defines [`Server`] which implements the MCP `ServerHandler`
//! trait and registers all 6 database tools using rmcp macros.

use crate::db::backend::Backend;
use crate::tools::database;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;

/// Request to list tables in a database.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    #[schemars(
        description = "The database name to list tables from. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
}

/// Request to get a table's schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    #[schemars(
        description = "The database name containing the table. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
    #[schemars(
        description = "The table name to inspect. Use list_tables first to see available tables in the database."
    )]
    pub table_name: String,
}

/// Request to execute a SQL query.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteSqlRequest {
    #[schemars(
        description = "Single SQL query to execute. In read-only mode only SELECT, SHOW, DESCRIBE, and USE are allowed. Caller should limit rows returned."
    )]
    pub sql_query: String,
    #[schemars(
        description = "The database to run the query against. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
}

/// Request to create a database.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    #[schemars(
        description = "Name of the database to create. Must contain only alphanumeric characters and underscores."
    )]
    pub database_name: String,
}

fn map_error(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// MCP server backed by a database backend.
#[derive(Clone)]
pub struct Server {
    pub backend: Backend,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl Server {
    /// Creates a new MCP server with the given database backend.
    #[must_use]
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl Server {
    /// List all accessible databases on the connected database server.
    #[tool(
        description = "List all accessible databases on the connected database server. Call this first to discover available database names."
    )]
    pub async fn list_databases(&self) -> Result<CallToolResult, ErrorData> {
        let result = database::list_databases(&self.backend)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// List all tables in a specific database.
    #[tool(
        description = "List all tables in a specific database. Requires database_name from list_databases."
    )]
    pub async fn list_tables(
        &self,
        req: Parameters<ListTablesRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = database::list_tables(&self.backend, &req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Get column definitions for a table.
    #[tool(
        description = "Get column definitions (type, nullable, key, default) for a table. Requires database_name and table_name."
    )]
    pub async fn get_table_schema(
        &self,
        req: Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result =
            database::get_table_schema(&self.backend, &req.0.database_name, &req.0.table_name)
                .await
                .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Get column definitions plus foreign key relationships for a table.
    #[tool(
        description = "Get column definitions plus foreign key relationships for a table. Requires database_name and table_name."
    )]
    pub async fn get_table_schema_with_relations(
        &self,
        req: Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = database::get_table_schema_with_relations(
            &self.backend,
            &req.0.database_name,
            &req.0.table_name,
        )
        .await
        .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Execute a SQL query against a specified database.
    #[tool(
        description = "Execute a SQL query. Requires database_name and sql_query. In read-only mode only SELECT, SHOW, DESCRIBE, USE are allowed."
    )]
    pub async fn execute_sql(
        &self,
        req: Parameters<ExecuteSqlRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result =
            database::tool_execute_sql(&self.backend, &req.0.sql_query, &req.0.database_name, None)
                .await
                .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Create a new database if it doesn't exist.
    #[tool(
        description = "Create a new database. Requires database_name. Blocked in read-only mode. Not supported for SQLite."
    )]
    pub async fn create_database(
        &self,
        req: Parameters<CreateDatabaseRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = database::create_database(&self.backend, &req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Database MCP Server - provides database exploration and query tools for MySQL, PostgreSQL, and SQLite",
            )
    }
}
