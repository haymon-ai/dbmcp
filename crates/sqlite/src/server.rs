//! MCP handler for the `SQLite` backend.
//!
//! Implements [`Backend`] on [`SqliteBackend`] to register
//! SQLite-specific MCP tools.

use database_mcp_server::{Backend, Server};
use rmcp::handler::server::tool::ToolRouter;

use super::SqliteBackend;
use super::tools::{GetTableSchemaTool, ListTablesTool, ReadQueryTool, WriteQueryTool};

impl Backend for SqliteBackend {
    fn provide_tool_router(&self) -> ToolRouter<Server<Self>> {
        let router = ToolRouter::new()
            .with_async_tool::<ListTablesTool>()
            .with_async_tool::<GetTableSchemaTool>()
            .with_async_tool::<ReadQueryTool>();

        if self.config.read_only {
            return router;
        }

        router.with_async_tool::<WriteQueryTool>()
    }
}
