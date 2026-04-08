//! Request and response types for MCP tool parameters.
//!
//! Each struct maps to the JSON input or output schema of one MCP tool.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Response for tools with no structured return data.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MessageResponse {
    /// Description of the completed operation.
    pub message: String,
}

/// Response for the `list_databases` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListDatabasesResponse {
    /// Sorted list of database names.
    pub databases: Vec<String>,
}

/// Request for the `create_database` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    /// Name of the database to create. Must contain only alphanumeric characters and underscores.
    pub database_name: String,
}

/// Request for the `drop_database` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct DropDatabaseRequest {
    /// Name of the database to drop. Must contain only alphanumeric characters and underscores.
    pub database_name: String,
}

/// Request for the `list_tables` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    /// The database name to list tables from. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
}

/// Response for the `list_tables` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListTablesResponse {
    /// Sorted list of table names.
    pub tables: Vec<String>,
}

/// Request for the `get_table_schema` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    /// The database name containing the table. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
    /// The table name to inspect. Use `list_tables` first to see available tables in the database.
    pub table_name: String,
}

/// Response for the `get_table_schema` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TableSchemaResponse {
    /// Name of the inspected table.
    pub table_name: String,
    /// Column definitions keyed by column name.
    pub columns: Value,
}

/// Request for the `read_query` and `write_query` tools.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// The database to run the query against. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
}

/// Response for the `read_query`, `write_query`, and `explain_query` tools.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QueryResponse {
    /// Result rows, each a JSON object keyed by a column name.
    pub rows: Value,
}

/// Request for the `explain_query` tool.
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
