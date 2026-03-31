//! `PostgreSQL` backend crate.
//!
//! Provides [`PostgresBackend`] for database operations and
//! [`PostgresHandler`] implementing the MCP `ServerHandler` trait.

mod connection;
mod handler;
mod operations;
mod schema;

pub use connection::PostgresBackend;
pub use handler::PostgresHandler;
