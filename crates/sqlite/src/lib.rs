//! `SQLite` backend crate.
//!
//! Provides [`SqliteHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod connection;
mod handler;
mod tools;
pub mod types;

pub use handler::SqliteHandler;
