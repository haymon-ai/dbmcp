//! MySQL/MariaDB backend crate.
//!
//! Provides [`MysqlHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod cache;
mod connection;
mod handler;
mod tools;
pub mod types;

pub use handler::MysqlHandler;
