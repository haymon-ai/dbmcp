//! Shared MCP server wrapper and metadata.
//!
//! Provides the cloneable, type-erased [`Server`] consumed by the
//! stdio and HTTP transports, plus [`server_info`] used by per-backend
//! [`ServerHandler`] implementations.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};

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
/// Wraps any backend adapter behind an [`Arc`] using the private
/// [`DynHandler`] trait for type erasure. All database backends produce the same concrete
/// type, eliminating the need for enum dispatch. The same `Arc` is held
/// a second time as a [`Shutdown`] view so transports can drain pools
/// via [`Server::shutdown`].
#[derive(Clone)]
pub struct Server {
    handler: Arc<dyn DynHandler>,
    shutdown: Arc<dyn Shutdown>,
}

/// Boxed `Send` future returned by [`DynHandler`] request methods.
type HandlerFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ErrorData>> + Send + 'a>>;

/// Object-safe view of the [`ServerHandler`] methods backends override.
///
/// [`ServerHandler`] returns `impl Future`, so it cannot be a trait object.
/// Every method not listed here keeps rmcp's default on [`Server`], which
/// matches what each backend inherits today.
trait DynHandler: Send + Sync {
    fn get_info(&self) -> ServerConfig;

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> HandlerFuture<'_, CallToolResponse>;

    fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> HandlerFuture<'_, ListToolsResult>;

    fn get_tool(&self, name: &str) -> Option<Tool>;
}

impl<H: ServerHandler> DynHandler for H {
    fn get_info(&self) -> ServerConfig {
        ServerHandler::get_info(self)
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> HandlerFuture<'_, CallToolResponse> {
        Box::pin(ServerHandler::call_tool(self, request, context))
    }

    fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> HandlerFuture<'_, ListToolsResult> {
        Box::pin(ServerHandler::list_tools(self, request, context))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        ServerHandler::get_tool(self, name)
    }
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
    /// One [`Arc`] is shared as two trait-object views — [`DynHandler`]
    /// for request handling and [`Shutdown`] for pool draining — so the
    /// adapter itself is never cloned.
    pub fn new<S>(handler: S) -> Self
    where
        S: ServerHandler + Shutdown,
    {
        let handler = Arc::new(handler);
        Self {
            handler: handler.clone(),
            shutdown: handler,
        }
    }

    /// Closes every live connection pool. Idempotent.
    pub async fn shutdown(&self) {
        self.shutdown.shutdown().await;
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        self.handler.get_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.handler.call_tool(request, context).await
    }

    /// Lists the backend's tools without cache hints.
    ///
    /// rmcp's `tool_handler` default advertises `cacheScope: public` for a
    /// tool list that depends on server config. Clearing the hints lets rmcp
    /// fill the conservative `ttlMs: 0` / `cacheScope: private` instead.
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = self.handler.list_tools(request, context).await?;
        result.ttl_ms = None;
        result.cache_scope = None;
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.handler.get_tool(name)
    }
}

/// Returns the shared [`ServerConfig`] for all server implementations.
///
/// Builds base [`Implementation`] metadata (name, version, title, URL).
/// Backend handlers extend this with a backend-specific description
/// and instructions via the public fields on [`ServerConfig`].
#[must_use]
pub fn server_info() -> ServerConfig {
    let capabilities = ServerCapabilities::builder().enable_tools().build();

    let server_info = Implementation::new(NAME, VERSION)
        .with_title(TITLE)
        .with_website_url(HOMEPAGE);

    ServerConfig::new(capabilities).with_server_info(server_info)
}
