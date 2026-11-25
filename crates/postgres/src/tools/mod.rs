//! MCP tool definitions for the `PostgreSQL` backend.
//!
//! Each tool is defined in its own submodule as a ZST that implements
//! [`ToolBase`](rmcp::handler::server::router::tool::ToolBase) and
//! [`AsyncTool`](rmcp::handler::server::router::tool::AsyncTool).
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
