//! MCP server handler, tool definitions, and request types.
//!
//! Provides [`Server`] which implements the MCP `ServerHandler` trait,
//! generic over any [`backend::DatabaseBackend`] implementation.

pub mod handler;
pub mod server;
pub mod tools;
pub mod types;

pub use server::Server;
pub use types::{CreateDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
