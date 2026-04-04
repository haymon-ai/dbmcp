//! Stdio transport command.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use clap::Parser;
use rmcp::ServiceExt;
use rmcp::service::ServerInitializeError;
use tracing::info;

use crate::server::ServerHandler;

/// Runs the MCP server in stdio mode.
#[derive(Debug, Parser)]
pub struct StdioCommand;

impl StdioCommand {
    /// Starts the MCP server using stdio transport.
    ///
    /// Serves JSON-RPC over stdin/stdout using the provided service.
    ///
    /// # Errors
    ///
    /// Returns an error if the stdio transport fails to initialize or
    /// the server encounters a fatal protocol error.
    pub async fn execute(&self, handler: ServerHandler) -> Result<(), ServerInitializeError> {
        info!("Starting MCP server via stdio transport...");

        let transport = rmcp::transport::io::stdio();
        let running = handler.serve(transport).await?;

        running.waiting().await.ok();
        Ok(())
    }
}
