//! MySQL/MariaDB backend crate.
//!
//! Provides [`MysqlBackend`] for database operations and
//! [`MysqlHandler`] implementing the MCP `ServerHandler` trait.

mod connection;
mod handler;
mod operations;
mod schema;

pub use connection::MysqlBackend;
pub use handler::MysqlHandler;
