//! Stdio transport.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use dbmcp_server::Server;
use rmcp::ServiceExt;
use tracing::{error, info};

use crate::error::ErrorKind;
use crate::server::shutdown_signal;

/// Serves JSON-RPC over stdin/stdout until the client disconnects or a signal arrives.
///
/// # Errors
///
/// Returns an error if the stdio transport fails to initialize.
pub(crate) async fn serve(server: Server) -> Result<(), ErrorKind> {
    info!("Starting MCP server via stdio transport...");
    let transport = rmcp::transport::io::stdio();
    let running = server.serve(transport).await?;

    // Exit on stdin EOF (client disconnect) or Ctrl-C/SIGTERM. On a
    // signal the losing `waiting()` future drops, cancelling the
    // service via `RunningService`'s `Drop`.
    tokio::select! {
        res = running.waiting() => {
            if let Err(join_error) = res {
                error!("stdio server task terminated abnormally: {join_error}");
            }
        }
        () = shutdown_signal() => {}
    }
    Ok(())
}
