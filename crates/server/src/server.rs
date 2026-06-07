//! Shared MCP server wrapper and metadata.
//!
//! Provides the cloneable, type-erased [`Server`] consumed by the
//! stdio and HTTP transports, plus [`server_info`] used by per-backend
//! [`ServerHandler`](rmcp::ServerHandler) implementations.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::RoleServer;
use rmcp::Service;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::{DynService, NotificationContext, RequestContext};

/// Hardcoded product name matching the root binary crate.
const NAME: &str = "dbmcp";

/// The current version, derived from the workspace `Cargo.toml` at compile time.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Human-readable title for the MCP server.
const TITLE: &str = "Database MCP Server";

/// Website URL, derived from the workspace `Cargo.toml` at compile time.
const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");

/// Cloneable, type-erased MCP server.
///
/// Wraps any backend adapter behind an [`Arc`] using rmcp's [`DynService`]
/// for type erasure. All database backends produce the same concrete
/// type, eliminating the need for enum dispatch. The same `Arc` is held
/// a second time as a [`Shutdown`] view so transports can drain pools
/// via [`Server::shutdown`].
#[derive(Clone)]
pub struct Server {
    service: Arc<dyn DynService<RoleServer>>,
    shutdown: Arc<dyn Shutdown>,
}

/// Boxed `Send` future returned by [`Shutdown::shutdown`].
pub type ShutdownFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Closes every live connection pool the server owns.
///
/// Implemented by each backend handler so transports can drain pools
/// gracefully on shutdown, separate from the type-erased [`Server`].
pub trait Shutdown: Send + Sync {
    /// Awaits a clean close of all pools. Idempotent.
    fn shutdown(&self) -> ShutdownFuture<'_>;
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl Server {
    /// Creates a new server from any backend adapter.
    ///
    /// One [`Arc`] is shared as two trait-object views — [`DynService`]
    /// for request handling and [`Shutdown`] for pool draining — so the
    /// adapter itself is never cloned.
    pub fn new<S>(handler: S) -> Self
    where
        S: Service<RoleServer> + Shutdown + 'static,
    {
        let handler = Arc::new(handler);
        Self {
            service: handler.clone(),
            shutdown: handler,
        }
    }

    /// Closes every live connection pool. Idempotent.
    pub async fn shutdown(&self) {
        self.shutdown.shutdown().await;
    }
}

impl Service<RoleServer> for Server {
    fn handle_request(
        &self,
        request: <RoleServer as rmcp::service::ServiceRole>::PeerReq,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<<RoleServer as rmcp::service::ServiceRole>::Resp, rmcp::ErrorData>> + Send + '_
    {
        DynService::handle_request(self.service.as_ref(), request, context)
    }

    fn handle_notification(
        &self,
        notification: <RoleServer as rmcp::service::ServiceRole>::PeerNot,
        context: NotificationContext<RoleServer>,
    ) -> impl Future<Output = Result<(), rmcp::ErrorData>> + Send + '_ {
        DynService::handle_notification(self.service.as_ref(), notification, context)
    }

    fn get_info(&self) -> <RoleServer as rmcp::service::ServiceRole>::Info {
        DynService::get_info(self.service.as_ref())
    }
}

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
