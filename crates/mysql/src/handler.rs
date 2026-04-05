//! MCP handler for the MySQL/MariaDB backend.
//!
//! Implements [`ServerHandler`] directly on [`MysqlAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::MysqlAdapter;

/// Backend-specific description for MySQL/MariaDB.
const DESCRIPTION: &str = "Database MCP Server for MySQL and MariaDB";

/// Backend-specific instructions for MySQL/MariaDB.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_databases` to discover available databases.
2. Call `list_tables` with a `database_name` to see its tables.
3. Call `get_table_schema` with `database_name` and `table_name` to inspect columns, types, and foreign keys before writing queries.
4. Use `read_query` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
5. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `create_database` to create a new database.

## Constraints

- The `write_query` and `create_database` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for MysqlAdapter {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(INSTRUCTIONS.into());
        info
    }
}
