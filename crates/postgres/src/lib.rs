//! `PostgreSQL` backend crate.
//!
//! Provides [`PostgresHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod cache;
mod connection;
mod handler;
mod tools;
pub mod types;

pub use handler::PostgresHandler;
