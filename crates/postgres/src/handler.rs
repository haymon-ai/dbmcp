//! MCP handler for the `PostgreSQL` backend.
//!
//! Implements [`ServerHandler`] directly on [`PostgresAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::PostgresAdapter;

/// Backend-specific description for `PostgreSQL`.
const DESCRIPTION: &str = "Database MCP Server for PostgreSQL";

/// Backend-specific instructions for `PostgreSQL`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_databases` to discover available databases.
2. Call `list_tables` with a `database_name` to see its tables.
3. Call `get_table_schema` with `database_name` and `table_name` to inspect columns, types, and foreign keys before writing queries.
4. Use `read_query` for read-only SQL (SELECT, SHOW, EXPLAIN).
5. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `create_database` to create a new database.
7. Use `drop_database` to drop an existing database.

Tools accept an optional `database_name` parameter to query across databases without reconnecting.

## Constraints

- The `write_query`, `create_database`, and `drop_database` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for PostgresAdapter {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(INSTRUCTIONS.into());
        info
    }
}
