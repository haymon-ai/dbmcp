//! Shared MCP server utilities and request types.
//!
//! Provides [`types`] for tool request/response schemas,
//! [`pagination`] cursor helpers, the [`tool`] registry, and the
//! [`Server`] wrapper plus [`server_info`] used by per-backend servers.

pub mod pagination;
mod server;
pub mod tool;
pub mod types;

pub use pagination::{Cursor, Pager};
pub use server::{Server, server_info};
pub use tool::{ToolRouterExt, ToolSpec};
