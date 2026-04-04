//! MCP handler for the `PostgreSQL` backend.
//!
//! Implements [`Backend`] on [`PostgresBackend`] to register
//! PostgreSQL-specific MCP tools.

use database_mcp_server::{Backend, Server};
use rmcp::handler::server::tool::ToolRouter;

use super::PostgresBackend;
use super::tools::{
    CreateDatabaseTool, GetTableSchemaTool, ListDatabasesTool, ListTablesTool, ReadQueryTool, WriteQueryTool,
};

impl Backend for PostgresBackend {
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
