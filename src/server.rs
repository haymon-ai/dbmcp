//! Server creation and type-erased dispatch.
//!
//! Provides [`ServerHandler`], a cloneable, type-erased MCP server
//! that wraps any database backend. The [`create_handler`] function
//! constructs the appropriate adapter and returns a [`ServerHandler`]
//! without establishing a database connection.

use std::sync::Arc;

use database_mcp_config::{Config, DatabaseBackend};
use database_mcp_mysql::MysqlAdapter;
use database_mcp_postgres::PostgresAdapter;
use database_mcp_sqlite::SqliteAdapter;
use rmcp::RoleServer;
use rmcp::Service;
use rmcp::service::{DynService, NotificationContext, RequestContext, ServiceExt};

/// Cloneable, type-erased MCP server.
///
/// Wraps any backend adapter behind an [`Arc`] using rmcp's [`DynService`]
/// for type erasure. All database backends produce the same concrete
/// type, eliminating the need for enum dispatch.
#[derive(Clone)]
pub struct ServerHandler(Arc<dyn DynService<RoleServer>>);

impl ServerHandler {
    /// Creates a new handler from any backend adapter.
    pub fn new(server: impl ServiceExt<RoleServer>) -> Self {
        Self(Arc::from(server.into_dyn()))
    }
}

impl From<SqliteAdapter> for ServerHandler {
    fn from(adapter: SqliteAdapter) -> Self {
        Self::new(adapter)
    }
}

impl From<PostgresAdapter> for ServerHandler {
    fn from(adapter: PostgresAdapter) -> Self {
        Self::new(adapter)
    }
}

impl From<MysqlAdapter> for ServerHandler {
    fn from(adapter: MysqlAdapter) -> Self {
        Self::new(adapter)
    }
}

impl Service<RoleServer> for ServerHandler {
    fn handle_request(
        &self,
        request: <RoleServer as rmcp::service::ServiceRole>::PeerReq,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<<RoleServer as rmcp::service::ServiceRole>::Resp, rmcp::ErrorData>> + Send + '_
    {
        DynService::handle_request(self.0.as_ref(), request, context)
    }

    fn handle_notification(
        &self,
        notification: <RoleServer as rmcp::service::ServiceRole>::PeerNot,
        context: NotificationContext<RoleServer>,
    ) -> impl Future<Output = Result<(), rmcp::ErrorData>> + Send + '_ {
        DynService::handle_notification(self.0.as_ref(), notification, context)
    }

    fn get_info(&self) -> <RoleServer as rmcp::service::ServiceRole>::Info {
        DynService::get_info(self.0.as_ref())
    }
}

/// Creates a [`ServerHandler`] based on the configured database backend.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable.
#[must_use]
pub fn create_handler(config: &Config) -> ServerHandler {
    match config.database.backend {
        DatabaseBackend::Sqlite => SqliteAdapter::new(&config.database).into(),
        DatabaseBackend::Postgres => PostgresAdapter::new(&config.database).into(),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => MysqlAdapter::new(&config.database).into(),
    }
}
