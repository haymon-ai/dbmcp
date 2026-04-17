//! Shared MCP server utilities and request types.
//!
//! Provides request types, [`Server`] wrapper, and [`server_info`] used
//! by per-backend server implementations.

mod server;
pub mod types;

pub use server::{Server, server_info};
pub use types::{CreateDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
