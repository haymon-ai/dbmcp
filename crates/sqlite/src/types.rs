//! SQLite-specific MCP tool request types.
//!
//! Unlike `MySQL` and `PostgreSQL`, `SQLite` operates on a single file and
//! has no database selection. These types omit the `database_name`
//! field present in the shared server types.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request to get a table's schema.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    /// The table name to inspect. Use `list_tables` first to see available tables.
    pub table_name: String,
}

/// Request for `read_query` and `write_query` tools.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
}
