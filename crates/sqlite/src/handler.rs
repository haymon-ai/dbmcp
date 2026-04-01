//! MCP handler for the `SQLite` backend.
//!
//! [`SqliteHandler`] wraps [`SqliteBackend`] and implements
//! [`ServerHandler`] using rmcp tool macros.

use database_mcp_backend::types::{GetTableSchemaRequest, ListTablesRequest, QueryRequest};
use database_mcp_config::DatabaseConfig;
use database_mcp_server::tools;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorData;

use super::SqliteBackend;

/// MCP handler for `SQLite` databases.
///
/// Owns a [`SqliteBackend`] and a pre-filtered [`ToolRouter`].
/// Write tools are removed when the backend is in read-only mode.
#[derive(Clone, Debug)]
pub struct SqliteHandler {
    backend: SqliteBackend,
    tool_router: ToolRouter<Self>,
}

impl SqliteHandler {
    /// Creates a new `SQLite` handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the database connection cannot be established.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, database_mcp_backend::AppError> {
        let backend = SqliteBackend::new(config).await?;
        let mut tool_router = Self::tool_router();
        if backend.read_only {
            tool_router.remove_route("write_query");
        }
        Ok(Self { backend, tool_router })
    }
}

#[rmcp::tool_router]
impl SqliteHandler {
    /// List all tables in a specific database.
    #[rmcp::tool(
        name = "list_tables",
        description = "List all tables in a specific database. Requires database_name from list_databases.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_tables(&self, Parameters(req): Parameters<ListTablesRequest>) -> Result<String, ErrorData> {
        tools::list_tables(self.backend.list_tables(&req.database_name), &req.database_name).await
    }

    /// Get column definitions for a table.
    #[rmcp::tool(
        name = "get_table_schema",
        description = "Get column definitions (type, nullable, key, default) and foreign key relationships for a table. Requires database_name and table_name.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_table_schema(&self, Parameters(req): Parameters<GetTableSchemaRequest>) -> Result<String, ErrorData> {
        tools::get_table_schema(
            self.backend.get_table_schema(&req.database_name, &req.table_name),
            &req.database_name,
            &req.table_name,
        )
        .await
    }

    /// Execute a read-only SQL query.
    #[rmcp::tool(
        name = "read_query",
        description = "Execute a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn read_query(&self, Parameters(req): Parameters<QueryRequest>) -> Result<String, ErrorData> {
        let db = tools::resolve_database(&req.database_name);
        tools::read_query(
            self.backend.execute_query(&req.sql_query, db),
            &req.sql_query,
            &req.database_name,
            |sql| {
                database_mcp_backend::validation::validate_read_only_with_dialect(
                    sql,
                    &sqlparser::dialect::SQLiteDialect {},
                )
            },
        )
        .await
    }

    /// Execute a write SQL query.
    #[rmcp::tool(
        name = "write_query",
        description = "Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn write_query(&self, Parameters(req): Parameters<QueryRequest>) -> Result<String, ErrorData> {
        let db = tools::resolve_database(&req.database_name);
        tools::write_query(
            self.backend.execute_query(&req.sql_query, db),
            &req.sql_query,
            &req.database_name,
        )
        .await
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
impl ServerHandler for SqliteHandler {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        database_mcp_server::server_info()
    }
}
