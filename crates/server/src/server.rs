//! Shared MCP server utilities.
//!
//! Provides [`server_info`] used by per-backend
//! [`ServerHandler`](rmcp::ServerHandler) implementations.

use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};

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
