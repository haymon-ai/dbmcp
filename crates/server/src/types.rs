//! Request types for MCP tool parameters.
//!
//! Each struct maps to the JSON input schema of one MCP tool.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request to list tables in a database.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    /// The database name to list tables from. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
}

/// Request to get a table's schema.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    /// The database name containing the table. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
    /// The table name to inspect. Use `list_tables` first to see available tables in the database.
    pub table_name: String,
}

/// Request for `read_query` and `write_query` tools.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// The database to run the query against. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
}

/// Request to create a database.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    /// Name of the database to create. Must contain only alphanumeric characters and underscores.
    pub database_name: String,
}

/// Request to drop a database.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct DropDatabaseRequest {
    /// Name of the database to drop. Must contain only alphanumeric characters and underscores.
    pub database_name: String,
}

/// Request to explain a query's execution plan.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ExplainQueryRequest {
    /// The database to explain against. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
    /// The SQL query to explain.
    pub query: String,
    /// If true, use EXPLAIN ANALYZE for actual execution statistics. In read-only mode, only allowed for read-only statements. Defaults to false.
    #[serde(default)]
    pub analyze: bool,
}
