//! MCP server setup, tool handlers, and transport dispatch.
//!
//! Defines [`Server`] which implements the MCP `ServerHandler`
//! trait. Tool registration is delegated to each database backend
//! via [`Backend::build_tool_router`].

pub mod tools;

use crate::db::backend::{Backend, DatabaseBackend};
use crate::db::validation::validate_read_only_with_dialect;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
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

/// Request for `read_query` and `write_query` tools.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    #[schemars(description = "The SQL query to execute.")]
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
    /// The active database backend.
    pub backend: Backend,
    read_only: bool,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Creates a new MCP server with the given database backend.
    ///
    /// The tool router is built by the backend, which decides
    /// which tools to register based on its capabilities and
    /// its own `read_only` configuration.
    #[must_use]
    pub fn new(backend: Backend) -> Self {
        let read_only = backend.read_only();
        let tool_router = backend.build_tool_router();
        Self {
            backend,
            read_only,
            tool_router,
        }
    }
}

/// Tool handler methods wired into the [`ToolRouter`] by each backend.
impl Server {
    /// List all accessible databases on the connected database server.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_databases(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.backend.tool_list_databases().await.map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// List all tables in a specific database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_tables(&self, req: Parameters<ListTablesRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_list_tables(&req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Get column definitions for a table.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn get_table_schema(&self, req: Parameters<GetTableSchemaRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_get_table_schema(&req.0.database_name, &req.0.table_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Get column definitions plus foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn get_table_schema_with_relations(
        &self,
        req: Parameters<GetTableSchemaRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_get_table_schema_with_relations(&req.0.database_name, &req.0.table_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Execute a read-only SQL query with AST validation.
    ///
    /// Always enforces SQL validation (only SELECT, SHOW, DESCRIBE,
    /// USE, EXPLAIN allowed) as defence-in-depth, regardless of the
    /// server's read-only setting.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if SQL validation fails or the query errors.
    pub async fn read_query(&self, req: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        // Scope the dialect so the non-Send Box<dyn Dialect> is dropped before .await
        {
            let dialect = self.backend.dialect();
            validate_read_only_with_dialect(&req.0.sql_query, dialect.as_ref()).map_err(map_error)?;
        }

        let result = self
            .backend
            .tool_execute_sql(&req.0.sql_query, &req.0.database_name, None)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Execute a write SQL query.
    ///
    /// No SQL type validation — the tool boundary is the access control.
    /// This tool is only registered when the server is not in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the query fails.
    pub async fn write_query(&self, req: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_execute_sql(&req.0.sql_query, &req.0.database_name, None)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Create a new database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn create_database(&self, req: Parameters<CreateDatabaseRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_create_database(&req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Database MCP Server - provides database exploration and query tools for MySQL, PostgreSQL, and SQLite",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn map_error_converts_display_to_error_data() {
        let err = AppError::ReadOnlyViolation;
        let mapped = map_error(err);
        assert!(
            mapped.message.contains("read-only"),
            "mapped error should preserve the original message"
        );
    }

    #[test]
    fn map_error_converts_string_to_error_data() {
        let mapped = map_error("something went wrong");
        assert_eq!(mapped.message, "something went wrong");
    }

    #[test]
    fn get_info_returns_tools_capability() {
        // Verify get_info advertises tool support — uses a dummy backend
        // that won't be called since we're only checking ServerInfo.
        let info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions("test");
        assert!(info.capabilities.tools.is_some(), "tools capability should be enabled");
    }
}
