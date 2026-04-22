//! PostgreSQL-specific MCP tool request types.
//!
//! These types include PostgreSQL-only parameters like `cascade`
//! that are not available on other backends.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropTableRequest {
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table: String,
    /// If true, use CASCADE to also drop dependent foreign key constraints. Defaults to false.
    #[serde(default)]
    pub cascade: bool,
}
