//! MySQL/MariaDB backend crate.
//!
//! Provides [`MysqlHandler`] for database operations with MCP
//! tool registration via [`ServerHandler`](rmcp::ServerHandler).

mod connection;
mod flavour;
mod handler;
mod policy;
mod queries;
mod tools;
pub mod types;

pub use handler::MysqlHandler;
