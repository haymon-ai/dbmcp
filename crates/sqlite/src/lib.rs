//! `SQLite` backend crate.
//!
//! Provides [`SqliteBackend`] for database operations and
//! [`SqliteHandler`] implementing the MCP `ServerHandler` trait.

mod connection;
mod handler;
mod operations;
mod schema;

pub use connection::SqliteBackend;
pub use handler::SqliteHandler;
