//! MySQL/MariaDB backend crate.
//!
//! Provides [`MysqlAdapter`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod adapter;
mod handler;
mod operations;
mod schema;
mod tools;

pub use adapter::MysqlAdapter;
