//! Shared MCP server wrapper and metadata.
//!
//! Provides the cloneable, type-erased [`Server`] consumed by the
//! stdio and HTTP transports, plus [`server_info`] used by per-backend
//! [`ServerHandler`] implementations.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
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
///
/// Tool execution failures (internal errors raised by a tool) are returned
/// to the client as a tool result with `isError: true` rather than a
/// JSON-RPC error, so the caller sees the message and can self-correct.
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
        into_tool_error(self.handler.call_tool(request, context).await)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.handler.list_tools(request, context).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.handler.get_tool(name)
    }
}

/// Converts a tool's internal error into an in-band `isError` tool result.
///
/// Mirrors rmcp's handling of argument errors: the tool was routed and ran,
/// so its failure belongs in the result. Every other error code (e.g. an
/// unknown tool) stays a JSON-RPC protocol error.
fn into_tool_error(result: Result<CallToolResponse, ErrorData>) -> Result<CallToolResponse, ErrorData> {
    match result {
        Err(error) if error.code == ErrorCode::INTERNAL_ERROR => {
            Ok(CallToolResult::error(vec![ContentBlock::text(error.message)]).into())
        }
        other => other,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_becomes_tool_result_with_is_error() {
        let result = into_tool_error(Err(ErrorData::internal_error("syntax error near FROM", None)));

        let Ok(CallToolResponse::Complete(result)) = result else {
            panic!("expected a completed tool result, got {result:?}");
        };
        assert_eq!(result.is_error, Some(true));
        let text = result.content[0].as_text().expect("text content");
        assert_eq!(text.text, "syntax error near FROM");
    }

    #[test]
    fn other_errors_stay_protocol_errors() {
        let result = into_tool_error(Err(ErrorData::invalid_params("tool not found", None)));

        let error = result.expect_err("invalid params must stay a protocol error");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn success_passes_through() {
        let result = into_tool_error(Ok(CallToolResult::success(vec![]).into()));

        let Ok(CallToolResponse::Complete(result)) = result else {
            panic!("expected a completed tool result, got {result:?}");
        };
        assert_ne!(result.is_error, Some(true));
    }
}
