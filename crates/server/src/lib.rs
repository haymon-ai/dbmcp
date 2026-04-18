//! Shared MCP server utilities and request types.
//!
//! Provides request types, [`pagination`] cursor helpers, [`Server`]
//! wrapper, and [`server_info`] used by per-backend server implementations.

pub mod pagination;
mod server;
pub mod types;

pub use pagination::{Cursor, PAGE_SIZE};
pub use server::{Server, server_info};
pub use types::{CreateDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
