//! SQLite-specific MCP tool request types.
//!
//! Unlike `MySQL` and `PostgreSQL`, `SQLite` operates on a single file and
//! has no database selection. These types omit the `database_name`
//! field present in the shared server types.

use database_mcp_server::pagination::Cursor;
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Request for the `get_table_schema` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    /// The table name to inspect. Use `list_tables` first to see available tables.
    pub table_name: String,
}

/// Request for the `drop_table` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct DropTableRequest {
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table_name: String,
}

/// Request for the `list_tables` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Request for the `write_query` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
}

/// Request for the `read_query` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ReadQueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    /// Ignored for `EXPLAIN` statements.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Request for the `explain_query` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ExplainQueryRequest {
    /// The SQL query to explain.
    pub query: String,
}
