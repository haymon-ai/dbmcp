//! Shared MCP server utilities and request types.
//!
//! Provides [`types`] for tool request/response schemas,
//! [`listing`] cursor pagination, the [`tool`] registry, [`schema`] input
//! and output schema generators, [`truncate`] character caps for query
//! rows, and the [`Server`] wrapper plus [`server_info`] used by
//! per-backend servers.

pub mod listing;
pub mod schema;
mod server;
pub mod tool;
pub mod truncate;
pub mod types;

pub use schema::{input_schema, output_schema};
pub use server::{PoolFuture, Pools, Server, server_info};
pub use tool::{ErrorContext, ToolHandler, ToolRouterExt, ToolSpec};
