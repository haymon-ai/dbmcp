//! Shared MCP server utilities.
//!
//! Provides [`server_info`] and [`map_error`] used by per-backend
//! [`ServerHandler`](rmcp::ServerHandler) implementations.

use rmcp::model::{ErrorData, Implementation, ServerCapabilities, ServerInfo};

/// Converts a displayable error into an MCP [`ErrorData`].
pub fn map_error(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Returns the shared [`ServerInfo`] for all server implementations.
///
/// Provides consistent server identity, capabilities, and instructions
/// across all database backends.
#[must_use]
pub fn server_info() -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Database MCP Server - provides database exploration and query tools for MySQL, MariaDB, PostgreSQL, and SQLite",
        )
}
