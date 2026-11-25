//! `SQLite` backend crate.
//!
//! Provides [`SqliteHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod connection;
mod handler;
mod operations;
mod policy;
mod queries;
mod tools;
pub mod types;

pub use handler::SqliteHandler;
