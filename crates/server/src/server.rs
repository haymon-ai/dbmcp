//! Shared MCP server utilities.
//!
//! Provides [`server_info`] used by per-backend
//! [`ServerHandler`](rmcp::ServerHandler) implementations and the
//! binary crate's `ServerHandler` wrapper.

use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};

/// Hardcoded product name matching the root binary crate.
const NAME: &str = "database-mcp";

/// The current version, derived from the workspace `Cargo.toml` at compile time.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Human-readable title for the MCP server.
const TITLE: &str = "Database MCP Server";

/// Website URL, derived from the workspace `Cargo.toml` at compile time.
const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");

/// Returns the shared [`ServerInfo`] for all server implementations.
///
/// Builds base [`Implementation`] metadata (name, version, title, URL).
/// Backend handlers extend this with a backend-specific description
/// and instructions via the public fields on [`ServerInfo`].
#[must_use]
pub fn server_info() -> ServerInfo {
    let capabilities = ServerCapabilities::builder().enable_tools().build();

    let server_info = Implementation::new(NAME, VERSION)
        .with_title(TITLE)
        .with_website_url(HOMEPAGE);

    ServerInfo::new(capabilities).with_server_info(server_info)
}
