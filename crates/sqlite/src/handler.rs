//! MCP handler for the `SQLite` backend.
//!
//! Implements [`ServerHandler`] directly on [`SqliteAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::SqliteAdapter;

/// Backend-specific description for `SQLite`.
const DESCRIPTION: &str = "Database MCP Server for SQLite";

/// Backend-specific instructions for `SQLite`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_tables` to discover tables in the connected database.
2. Call `get_table_schema` with a `table_name` to inspect columns, types, and foreign keys before writing queries.
3. Use `read_query` for read-only SQL (SELECT, EXPLAIN).
4. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

## Constraints

- The `write_query` tool is hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for SqliteAdapter {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(INSTRUCTIONS.into());
        info
    }
}
