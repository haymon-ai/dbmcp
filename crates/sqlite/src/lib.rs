//! `SQLite` backend crate.
//!
//! Provides [`SqliteAdapter`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod adapter;
mod handler;
mod operations;
mod schema;
mod tools;
pub mod types;

pub use adapter::SqliteAdapter;
