//! MCP handler for the `PostgreSQL` backend.
//!
//! Implements [`ServerHandler`] directly on [`PostgresAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::PostgresAdapter;

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for PostgresAdapter {
    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}
