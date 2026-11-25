//! `PostgreSQL` backend crate.
//!
//! Provides [`PostgresHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod connection;
mod handler;
mod policy;
mod queries;
mod tools;
pub mod types;
mod version;

pub use handler::PostgresHandler;
