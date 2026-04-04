//! MCP handler for the MySQL/MariaDB backend.
//!
//! Implements [`Backend`] on [`MysqlBackend`] to register
//! MySQL-specific MCP tools.

use database_mcp_server::{Backend, Server};
use rmcp::handler::server::tool::ToolRouter;

use super::MysqlBackend;
use super::tools::{
    CreateDatabaseTool, GetTableSchemaTool, ListDatabasesTool, ListTablesTool, ReadQueryTool, WriteQueryTool,
};

impl Backend for MysqlBackend {
    fn provide_tool_router(&self) -> ToolRouter<Server<Self>> {
        let router = ToolRouter::new()
            .with_async_tool::<ListDatabasesTool>()
            .with_async_tool::<ListTablesTool>()
            .with_async_tool::<GetTableSchemaTool>()
            .with_async_tool::<ReadQueryTool>();

        if self.config.read_only {
            return router;
        }

        router
            .with_async_tool::<WriteQueryTool>()
            .with_async_tool::<CreateDatabaseTool>()
    }
}
