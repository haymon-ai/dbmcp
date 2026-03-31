//! MCP server struct and constructor.
//!
//! Defines [`Server`] which holds the database backend and tool router.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ErrorData, Tool};

use crate::tools::build_tool_router;

/// Converts a displayable error into an MCP [`ErrorData`].
pub(crate) fn map_error(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// MCP server backed by a database backend.
#[derive(Clone)]
pub struct Server<B: backend::DatabaseBackend> {
    /// The active database backend.
    pub backend: B,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl<B: backend::DatabaseBackend> std::fmt::Debug for Server<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl<B: backend::DatabaseBackend + 'static> Server<B> {
    /// Creates a new MCP server with the given database backend.
    ///
    /// The tool router is built based on the backend's capabilities
    /// and read-only setting.
    #[must_use]
    pub fn new(backend: B) -> Self {
        let tool_router = build_tool_router(&backend);
        Self { backend, tool_router }
    }

    /// Looks up a tool by name in the router.
    pub fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}
