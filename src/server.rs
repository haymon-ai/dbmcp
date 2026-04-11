//! Backend-selection factory for the type-erased MCP server.
//!
//! The cloneable [`Server`] wrapper itself lives in
//! [`database_mcp_server`]; this module only owns [`create_server`],
//! which maps a configured [`DatabaseBackend`] onto the matching
//! concrete adapter.

use database_mcp_config::{Config, DatabaseBackend};
use database_mcp_mysql::MysqlHandler;
use database_mcp_postgres::PostgresHandler;
use database_mcp_sqlite::SqliteHandler;

pub use database_mcp_server::Server;

/// Creates a [`Server`] based on the configured database backend.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable.
#[must_use]
pub fn create_server(config: &Config) -> Server {
    match config.database.backend {
        DatabaseBackend::Sqlite => SqliteHandler::new(&config.database).into(),
        DatabaseBackend::Postgres => PostgresHandler::new(&config.database).into(),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => MysqlHandler::new(&config.database).into(),
    }
}
