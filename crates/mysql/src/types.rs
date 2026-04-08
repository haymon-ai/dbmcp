//! MySQL/MariaDB-specific MCP tool request types.
//!
//! These types omit PostgreSQL-only parameters like `cascade`.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request for the `drop_table` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct DropTableRequest {
    /// The database containing the table. Required. Use `list_databases` first to see available databases.
    pub database_name: String,
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table_name: String,
}
