//! Shared helpers for approval tests.
//!
//! Provides duplex transport setup and client lifecycle management
//! used by tool schema snapshot tests.

use rmcp::service::{Peer, RunningService, ServiceExt};

/// Connects an MCP adapter over a duplex transport, runs a closure, then cleans up.
///
/// Handles the full client lifecycle: duplex creation, server spawn, closure
/// execution, client cancellation, and server join.
pub async fn run_with_client<S, F, Fut>(adapter: S, f: F)
where
    S: ServiceExt<rmcp::RoleServer> + Send + 'static,
    F: FnOnce(Peer<rmcp::RoleClient>) -> Fut,
    Fut: Future<Output = ()>,
{
    let (server_transport, client_transport) = tokio::io::duplex(4096);

    let server_handle = tokio::spawn(async move {
        let running = adapter.serve(server_transport).await.expect("server serve failed");
        running.waiting().await.ok();
    });

    let client: RunningService<rmcp::RoleClient, ()> = ().serve(client_transport).await.expect("client serve failed");

    f(client.peer().clone()).await;

    client.cancel().await.expect("client cancel failed");
    server_handle.await.expect("server task failed");
}
