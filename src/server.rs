//! MCP server setup, tool registration, and transport dispatch.
//!
//! Defines [`DbMcpServer`] which implements the MCP `ServerHandler`
//! trait and registers all 6 database tools using rmcp macros.

use crate::db::backend::Backend;
use crate::tools::database;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;

/// Parameters for listing tables in a database.
#[derive(Deserialize, JsonSchema)]
pub struct DatabaseNameParam {
    /// Name of the database.
    pub database_name: String,
}

/// Parameters for inspecting a table's schema.
#[derive(Deserialize, JsonSchema)]
pub struct TableSchemaParam {
    /// Name of the database.
    pub database_name: String,
    /// Name of the table.
    pub table_name: String,
}

/// Parameters for executing a SQL query.
#[derive(Deserialize, JsonSchema)]
pub struct ExecuteSqlParam {
    /// The SQL query to execute.
    pub sql_query: String,
    /// Target database name.
    pub database_name: String,
}

/// MCP server backed by a database backend trait.
#[derive(Clone)]
pub struct DbMcpServer {
    pub backend: Backend,
    tool_router: ToolRouter<Self>,
}

impl DbMcpServer {
    /// Creates a new MCP server with the given database backend.
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl DbMcpServer {
    /// List all accessible databases.
    #[tool(description = "List all accessible databases on the connected database server")]
    pub async fn list_databases(&self) -> Result<String, String> {
        database::list_databases(&self.backend)
            .await
            .map_err(|e| e.to_string())
    }

    /// List all tables in a specific database.
    #[tool(description = "List all tables in a specific database")]
    pub async fn list_tables(
        &self,
        params: Parameters<DatabaseNameParam>,
    ) -> Result<String, String> {
        database::list_tables(&self.backend, &params.0.database_name)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get the schema of a specific table including column details.
    #[tool(description = "Get the schema of a specific table including column details")]
    pub async fn get_table_schema(
        &self,
        params: Parameters<TableSchemaParam>,
    ) -> Result<String, String> {
        database::get_table_schema(&self.backend, &params.0.database_name, &params.0.table_name)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get table schema with foreign key relationships.
    #[tool(description = "Get table schema with foreign key relationships")]
    pub async fn get_table_schema_with_relations(
        &self,
        params: Parameters<TableSchemaParam>,
    ) -> Result<String, String> {
        database::get_table_schema_with_relations(
            &self.backend,
            &params.0.database_name,
            &params.0.table_name,
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// Execute a SQL query against a specified database.
    #[tool(description = "Execute a SQL query against a specified database")]
    pub async fn execute_sql(&self, params: Parameters<ExecuteSqlParam>) -> Result<String, String> {
        database::tool_execute_sql(
            &self.backend,
            &params.0.sql_query,
            &params.0.database_name,
            None,
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// Create a new database if it doesn't exist.
    #[tool(description = "Create a new database if it doesn't exist")]
    pub async fn create_database(
        &self,
        params: Parameters<DatabaseNameParam>,
    ) -> Result<String, String> {
        database::create_database(&self.backend, &params.0.database_name)
            .await
            .map_err(|e| e.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DbMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Database MCP Server - provides database exploration and query tools for MySQL, PostgreSQL, and SQLite",
            )
    }
}
