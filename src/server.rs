//! Server handler dispatch.
//!
//! Contains the unified [`Handler`] enum that wraps all database
//! backend handlers and dispatches [`ServerHandler`] calls to the
//! active backend.

use database_mcp_config::{Config, DatabaseBackend};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorData, ListToolsResult, PaginatedRequestParams, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};

/// Unified handler enum dispatching to the active backend.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Handler {
    /// `SQLite` file-based backend.
    Sqlite(database_mcp_sqlite::SqliteHandler),
    /// `PostgreSQL` backend.
    Postgres(database_mcp_postgres::PostgresHandler),
    /// `MySQL` / `MariaDB` backend.
    Mysql(database_mcp_mysql::MysqlHandler),
}

/// Delegates a [`ServerHandler`] method call to the inner handler.
macro_rules! dispatch {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Handler::Sqlite(h) => h.$method($($arg),*),
            Handler::Postgres(h) => h.$method($($arg),*),
            Handler::Mysql(h) => h.$method($($arg),*),
        }
    };
    (await $self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Handler::Sqlite(h) => h.$method($($arg),*).await,
            Handler::Postgres(h) => h.$method($($arg),*).await,
            Handler::Mysql(h) => h.$method($($arg),*).await,
        }
    };
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerInfo {
        dispatch!(self, get_info)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        dispatch!(await self, list_tools, request, context)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch!(await self, call_tool, request, context)
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        dispatch!(self, get_tool, name)
    }
}

/// Creates a [`Handler`] based on the configured database backend.
///
/// # Errors
///
/// Returns an error if the database connection fails (invalid URL,
/// unreachable host, authentication failure).
pub async fn create_handler(config: &Config) -> Result<Handler, database_mcp_backend::AppError> {
    let handler = match config.database.backend {
        DatabaseBackend::Sqlite => Handler::Sqlite(database_mcp_sqlite::SqliteHandler::new(&config.database).await?),
        DatabaseBackend::Postgres => {
            Handler::Postgres(database_mcp_postgres::PostgresHandler::new(&config.database).await?)
        }
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => {
            Handler::Mysql(database_mcp_mysql::MysqlHandler::new(&config.database).await?)
        }
    };
    Ok(handler)
}
