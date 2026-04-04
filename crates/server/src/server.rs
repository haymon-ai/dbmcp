//! Shared MCP server utilities.
//!
//! Provides [`server_info`] used by per-backend
//! [`ServerHandler`](rmcp::ServerHandler) implementations.

use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};

/// Returns the shared [`ServerInfo`] for all server implementations.
///
/// Provides consistent server identity and capabilities across all
/// database backends. Backends should call [`ServerInfo::with_instructions`]
/// to add backend-specific instructions.
#[must_use]
pub fn server_info() -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
}
