//! `ServerHandler` implementation for [`Server`].
//!
//! Bridges MCP protocol messages to database backend operations.

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use tracing::{error, info};

use crate::server::{Server, map_error};
use crate::types::{CreateDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};

// ---------------------------------------------------------------------------
// ServerHandler trait
// ---------------------------------------------------------------------------

impl<B: backend::DatabaseBackend + 'static> ServerHandler for Server<B> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Database MCP Server - provides database exploration and query tools for MySQL, MariaDB, PostgreSQL, and SQLite",
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

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.tool_router.get(name).cloned()
    }
}

// ---------------------------------------------------------------------------
// Tool handler methods
// ---------------------------------------------------------------------------

impl<B: backend::DatabaseBackend + 'static> Server<B> {
    /// List all accessible databases on the connected database server.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_databases(&self) -> Result<CallToolResult, ErrorData> {
        info!("TOOL: list_databases called");
        let db_list = self.backend.list_databases().await.map_err(map_error)?;
        info!("TOOL: list_databases completed. Databases found: {}", db_list.len());
        let json = serde_json::to_string_pretty(&db_list).unwrap_or_else(|_| "[]".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// List all tables in a specific database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_tables(&self, req: Parameters<ListTablesRequest>) -> Result<CallToolResult, ErrorData> {
        let database_name = &req.0.database_name;
        info!("TOOL: list_tables called. database_name={database_name}");
        let table_list = match self.backend.list_tables(database_name).await {
            Ok(t) => t,
            Err(e) => {
                error!("TOOL ERROR: list_tables failed for database_name={database_name}: {e}");
                return Err(map_error(e));
            }
        };
        info!("TOOL: list_tables completed. Tables found: {}", table_list.len());
        let json = serde_json::to_string_pretty(&table_list).unwrap_or_else(|_| "[]".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get column definitions and foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn get_table_schema(&self, req: Parameters<GetTableSchemaRequest>) -> Result<CallToolResult, ErrorData> {
        let database_name = &req.0.database_name;
        let table_name = &req.0.table_name;
        info!("TOOL: get_table_schema called. database_name={database_name}, table_name={table_name}");
        let schema = self
            .backend
            .get_table_schema(database_name, table_name)
            .await
            .map_err(map_error)?;
        info!("TOOL: get_table_schema completed");
        let json = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
        let sql_query = &req.0.sql_query;
        let database_name = &req.0.database_name;
        info!(
            "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
            &sql_query[..sql_query.len().min(100)]
        );

        // Scope the dialect so the non-Send Box<dyn Dialect> is dropped before .await
        {
            let dialect = self.backend.dialect();
            backend::validation::validate_read_only_with_dialect(sql_query, dialect.as_ref()).map_err(map_error)?;
        }

        let db = if database_name.is_empty() {
            None
        } else {
            Some(database_name.as_str())
        };

        let results = self.backend.execute_query(sql_query, db).await.map_err(map_error)?;
        let row_count = results.as_array().map_or(0, Vec::len);
        info!("TOOL: execute_sql completed. Rows returned: {row_count}");
        let json = serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
        let sql_query = &req.0.sql_query;
        let database_name = &req.0.database_name;
        info!(
            "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
            &sql_query[..sql_query.len().min(100)]
        );

        let db = if database_name.is_empty() {
            None
        } else {
            Some(database_name.as_str())
        };

        let results = self.backend.execute_query(sql_query, db).await.map_err(map_error)?;
        let row_count = results.as_array().map_or(0, Vec::len);
        info!("TOOL: execute_sql completed. Rows returned: {row_count}");
        let json = serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Create a new database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn create_database(&self, req: Parameters<CreateDatabaseRequest>) -> Result<CallToolResult, ErrorData> {
        let database_name = &req.0.database_name;
        info!("TOOL: create_database called for database: '{database_name}'");
        let result = self.backend.create_database(database_name).await.map_err(map_error)?;
        info!("TOOL: create_database completed");
        let json = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into());
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
