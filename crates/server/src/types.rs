//! Request and response types for MCP tool parameters.
//!
//! Each struct maps to the JSON input or output schema of one MCP tool.

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pagination::Cursor;

/// Response for tools with no structured return data.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    /// Description of the completed operation.
    pub message: String,
}

/// Request for the `listDatabases` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listDatabases` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesResponse {
    /// Sorted list of database names for this page.
    pub databases: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `createDatabase` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseRequest {
    /// Name of the database to create. Must contain only alphanumeric characters and underscores.
    pub database: String,
}

/// Request for the `dropDatabase` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropDatabaseRequest {
    /// Name of the database to drop. Must contain only alphanumeric characters and underscores.
    pub database: String,
}

/// Request for the `listTables` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesRequest {
    /// The database name to list tables from. Required. Use `listDatabases` first to see available databases.
    pub database: String,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listTables` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesResponse {
    /// Sorted list of table names for this page.
    pub tables: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `getTableSchema` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetTableSchemaRequest {
    /// The database name containing the table. Required. Use `listDatabases` first to see available databases.
    pub database: String,
    /// The table name to inspect. Use `listTables` first to see available tables in the database.
    pub table: String,
}

/// Response for the `getTableSchema` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableSchemaResponse {
    /// Name of the inspected table.
    pub table: String,
    /// Column definitions keyed by column name.
    pub columns: Value,
}

/// Request for the `writeQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// The database to run the query against. Required. Use `listDatabases` first to see available databases.
    pub database: String,
}

/// Request for the `readQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadQueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// The database to run the query against. Required. Use `listDatabases` first to see available databases.
    pub database: String,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `writeQuery` and `explainQuery` tools.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    /// Result rows, each a JSON object keyed by a column name.
    pub rows: Vec<Value>,
}

/// Response for the `readQuery` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadQueryResponse {
    /// Result rows, each a JSON object keyed by a column name.
    pub rows: Vec<Value>,
    /// Opaque cursor pointing to the next page. Absent when this is the final
    /// page, when the result fits in one page, or when the statement is a
    /// non-`SELECT` kind that does not paginate (e.g. `SHOW`, `EXPLAIN`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `explainQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExplainQueryRequest {
    /// The database to explain against. Required. Use `listDatabases` first to see available databases.
    pub database: String,
    /// The SQL query to explain.
    pub query: String,
    /// If true, use EXPLAIN ANALYZE for actual execution statistics. In read-only mode, only allowed for read-only statements. Defaults to false.
    #[serde(default)]
    pub analyze: bool,
}
