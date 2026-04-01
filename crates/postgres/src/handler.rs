//! MCP handler for the `PostgreSQL` backend.
//!
//! [`PostgresHandler`] wraps [`PostgresBackend`] and implements
//! [`ServerHandler`] using rmcp tool macros.

use database_mcp_backend::types::{CreateDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
use database_mcp_config::DatabaseConfig;
use database_mcp_server::tools;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorData;

use super::PostgresBackend;

/// MCP handler for `PostgreSQL` databases.
///
/// Owns a [`PostgresBackend`] and a pre-filtered [`ToolRouter`].
/// Write tools are removed when the backend is in read-only mode.
#[derive(Clone, Debug)]
pub struct PostgresHandler {
    backend: PostgresBackend,
    tool_router: ToolRouter<Self>,
}

impl PostgresHandler {
    /// Creates a new `PostgreSQL` handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the database connection cannot be established.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, database_mcp_backend::AppError> {
        let backend = PostgresBackend::new(config).await?;
        let mut tool_router = Self::tool_router();
        if backend.read_only {
            tool_router.remove_route("write_query");
            tool_router.remove_route("create_database");
        }
        Ok(Self { backend, tool_router })
    }
}

#[rmcp::tool_router]
impl PostgresHandler {
    /// List all accessible databases.
    #[rmcp::tool(
        name = "list_databases",
        description = "List all accessible databases on the connected database server. Call this first to discover available database names.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_databases(&self) -> Result<String, ErrorData> {
        tools::list_databases(self.backend.list_databases()).await
    }

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
        tools::read_query(
            self.backend
                .execute_query(&req.sql_query, tools::resolve_database(&req.database_name)),
            &req.sql_query,
            &req.database_name,
            |sql| {
                database_mcp_backend::validation::validate_read_only_with_dialect(
                    sql,
                    &sqlparser::dialect::PostgreSqlDialect {},
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
        tools::write_query(
            self.backend
                .execute_query(&req.sql_query, tools::resolve_database(&req.database_name)),
            &req.sql_query,
            &req.database_name,
        )
        .await
    }

    /// Create a new database.
    #[rmcp::tool(
        name = "create_database",
        description = "Create a new database. Not supported for SQLite.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_database(&self, Parameters(req): Parameters<CreateDatabaseRequest>) -> Result<String, ErrorData> {
        tools::create_database(self.backend.create_database(&req.database_name), &req.database_name).await
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
impl ServerHandler for PostgresHandler {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        database_mcp_server::server_info()
    }
}
