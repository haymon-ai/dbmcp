//! MCP handler for the `SQLite` backend.
//!
//! Implements [`ServerHandler`] directly on [`SqliteAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::SqliteAdapter;

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for SqliteAdapter {
    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}
