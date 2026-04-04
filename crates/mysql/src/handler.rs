//! MCP handler for the MySQL/MariaDB backend.
//!
//! Implements [`ServerHandler`] directly on [`MysqlAdapter`] using
//! rmcp macros for tool dispatch.

use database_mcp_server::server_info;
use rmcp::model::ServerInfo;

use super::MysqlAdapter;

#[rmcp::tool_handler(router = self.build_tool_router())]
impl rmcp::ServerHandler for MysqlAdapter {
    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}
