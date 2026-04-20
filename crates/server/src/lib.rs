//! Shared MCP server utilities and request types.
//!
//! Provides [`types`] for tool request/response schemas,
//! [`pagination`] cursor helpers, and the [`Server`] wrapper plus
//! [`server_info`] used by per-backend server implementations.

pub mod pagination;
mod server;
pub mod types;

pub use pagination::{Cursor, Pager};
pub use server::{Server, server_info};
