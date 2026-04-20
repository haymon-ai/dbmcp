//! `PostgreSQL` handler: composes a [`PostgresConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`PostgresConnection`]. This module wires the connection into
//! the MCP `ServerHandler` surface and exposes a small set of thin
//! delegator methods that the per-tool implementations call.

use database_mcp_config::DatabaseConfig;
use database_mcp_server::{Server, server_info};
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};

use crate::connection::PostgresConnection;

use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListDatabasesTool,
    ListTablesTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for `PostgreSQL`.
const DESCRIPTION: &str = "Database MCP Server for PostgreSQL";

/// Backend-specific instructions for `PostgreSQL`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `listDatabases` to discover available databases.
2. Call `listTables` with a `database` to see its tables.
3. Call `getTableSchema` with `database` and `table` to inspect columns, types, and foreign keys before writing queries.
4. Use `readQuery` for read-only SQL (SELECT, SHOW, EXPLAIN).
5. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
7. Use `createDatabase` to create a new database.
8. Use `dropDatabase` to drop an existing database.
9. Use `dropTable` to remove a table from a database (supports `cascade` for foreign key dependencies).

Tools accept an optional `database` parameter to query across databases without reconnecting.

## Constraints

- The `writeQuery`, `createDatabase`, `dropDatabase`, and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// `PostgreSQL` database handler.
///
/// Composes one [`PostgresConnection`] (which owns every pool
/// and the pool initialization logic) with the per-backend MCP tool
/// router.
#[derive(Clone)]
pub struct PostgresHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) connection: PostgresConnection,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PostgresHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresHandler")
            .field("read_only", &self.config.read_only)
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl PostgresHandler {
    /// Creates a new `PostgreSQL` handler.
    ///
    /// Constructs the [`PostgresConnection`] (which builds the
    /// lazy default pool and the per-database pool cache) and the MCP
    /// tool router. No network I/O happens here.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        Self {
            config: config.clone(),
            connection: PostgresConnection::new(config),
            tool_router: build_tool_router(config.read_only),
        }
    }
}

impl From<PostgresHandler> for Server {
    /// Wraps a [`PostgresHandler`] in the type-erased MCP server.
    fn from(handler: PostgresHandler) -> Self {
        Self::new(handler)
    }
}

/// Builds the tool router, including write tools only when not in read-only mode.
fn build_tool_router(read_only: bool) -> ToolRouter<PostgresHandler> {
    let mut router = ToolRouter::new()
        .with_async_tool::<ListDatabasesTool>()
        .with_async_tool::<ListTablesTool>()
        .with_async_tool::<GetTableSchemaTool>()
        .with_async_tool::<ReadQueryTool>()
        .with_async_tool::<ExplainQueryTool>();

    if !read_only {
        router = router
            .with_async_tool::<CreateDatabaseTool>()
            .with_async_tool::<DropDatabaseTool>()
            .with_async_tool::<DropTableTool>()
            .with_async_tool::<WriteQueryTool>();
    }
    router
}

impl ServerHandler for PostgresHandler {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(INSTRUCTIONS.into());
        info
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database_mcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Postgres,
            host: "pg.example.com".into(),
            port: 5433,
            user: "pgadmin".into(),
            password: Some("pgpass".into()),
            name: Some("mydb".into()),
            ..DatabaseConfig::default()
        }
    }

    fn handler(read_only: bool) -> PostgresHandler {
        PostgresHandler::new(&DatabaseConfig {
            read_only,
            ..base_config()
        })
    }

    #[tokio::test]
    async fn handler_exposes_connection_default_db() {
        let handler = PostgresHandler::new(&base_config());
        assert_eq!(handler.connection.default_database_name(), "mydb");
    }

    #[tokio::test]
    async fn router_exposes_all_nine_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "listDatabases",
            "listTables",
            "getTableSchema",
            "readQuery",
            "explainQuery",
            "createDatabase",
            "dropDatabase",
            "dropTable",
            "writeQuery",
        ] {
            assert!(router.has_route(name), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn router_hides_write_tools_in_read_only_mode() {
        let router = handler(true).tool_router;
        assert!(router.has_route("listDatabases"));
        assert!(router.has_route("listTables"));
        assert!(router.has_route("getTableSchema"));
        assert!(router.has_route("readQuery"));
        assert!(router.has_route("explainQuery"));
        assert!(!router.has_route("writeQuery"));
        assert!(!router.has_route("createDatabase"));
        assert!(!router.has_route("dropDatabase"));
        assert!(!router.has_route("dropTable"));
    }
}
