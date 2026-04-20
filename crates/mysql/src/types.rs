//! MySQL/MariaDB-specific MCP tool request types.
//!
//! These types omit PostgreSQL-only parameters like `cascade`.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropTableRequest {
    /// The database containing the table. Required. Use `listDatabases` first to see available databases.
    pub database: String,
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table: String,
}
