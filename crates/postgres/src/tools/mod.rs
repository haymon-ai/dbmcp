//! MCP tool shapes for the `PostgreSQL` backend.
//!
//! Each tool is a ZST in its own submodule that implements
//! [`ToolBase`](rmcp::handler::server::router::tool::ToolBase) and
//! [`AsyncTool`](rmcp::handler::server::router::tool::AsyncTool). Its `invoke`
//! forwards to the handler method in [`operations`](crate::operations).
//! Router assembly happens in [`crate::handler`].

mod explain;
mod objects;
mod prelude;
mod read;
mod write;

pub(crate) use explain::ExplainTool;
pub(crate) use objects::ObjectsTool;
pub(crate) use read::ReadTool;
pub(crate) use write::WriteTool;
