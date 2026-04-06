//! Shared MCP server utilities, error types, and request types.
//!
//! Provides [`AppError`], request types, and [`server_info`] used
//! by per-backend server implementations.

pub mod error;
mod server;
pub mod types;

pub use error::AppError;
pub use server::server_info;
pub use types::{CreateDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
