//! MySQL/MariaDB handler: composes a [`MysqlConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`MysqlConnection`]. This module exposes the MCP `ServerHandler`
//! surface and a small set of thin delegators that per-tool
//! implementations call.

use dbmcp_config::DatabaseConfig;
use dbmcp_server::{Server, server_info};
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};

use crate::connection::MysqlConnection;
use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListDatabasesTool,
    ListFunctionsTool, ListProceduresTool, ListTablesTool, ListTriggersTool, ListViewsTool, ReadQueryTool,
    WriteQueryTool,
};

/// Backend-specific description for MySQL/MariaDB.
const DESCRIPTION: &str = "Database MCP Server for MySQL and MariaDB";

/// Backend-specific instructions for MySQL/MariaDB.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `listDatabases` to discover available databases.
2. Call `listTables` to see tables.
3. Call `listViews` to see views.
4. Call `listTriggers` to see triggers.
5. Call `listFunctions` to see stored functions.
6. Call `listProcedures` to see stored procedures.
7. Call `getTableSchema` to inspect columns, types, and foreign keys before writing queries.
8. Use `readQuery` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
9. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
10. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
11. Use `createDatabase` to create a new database.
12. Use `dropDatabase` to drop an existing database.
13. Use `dropTable` to remove a table from a database.

Per-database tools default to the active database; pass `database` to target another.

## Constraints

- The `writeQuery`, `createDatabase`, `dropDatabase`, and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// MySQL/MariaDB database handler.
///
/// Composes one [`MysqlConnection`] (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct MysqlHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) connection: MysqlConnection,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MysqlHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlHandler")
            .field("read_only", &self.config.read_only)
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl MysqlHandler {
    /// Creates a new `MySQL` handler.
    ///
    /// Constructs the [`MysqlConnection`] (which builds the
    /// lazy pool) and the MCP tool router. No network I/O happens here.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        Self {
            config: config.clone(),
            connection: MysqlConnection::new(config),
            tool_router: build_tool_router(config.read_only),
        }
    }
}

impl From<MysqlHandler> for Server {
    /// Wraps a [`MysqlHandler`] in the type-erased MCP server.
    fn from(handler: MysqlHandler) -> Self {
        Self::new(handler)
    }
}

/// Builds the tool router, including write tools only when not in read-only mode.
fn build_tool_router(read_only: bool) -> ToolRouter<MysqlHandler> {
    let mut router = ToolRouter::new()
        .with_async_tool::<ListDatabasesTool>()
        .with_async_tool::<ListTablesTool>()
        .with_async_tool::<ListViewsTool>()
        .with_async_tool::<ListTriggersTool>()
        .with_async_tool::<ListFunctionsTool>()
        .with_async_tool::<ListProceduresTool>()
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

impl ServerHandler for MysqlHandler {
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
    use dbmcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Mysql,
            host: "db.example.com".into(),
            port: 3307,
            user: "admin".into(),
            password: Some("s3cret".into()),
            name: Some("mydb".into()),
            ..DatabaseConfig::default()
        }
    }

    fn handler(read_only: bool) -> MysqlHandler {
        MysqlHandler::new(&DatabaseConfig {
            read_only,
            ..base_config()
        })
    }

    #[tokio::test]
    async fn router_exposes_all_thirteen_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "listDatabases",
            "listTables",
            "listViews",
            "listTriggers",
            "listFunctions",
            "listProcedures",
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
        assert!(router.has_route("listViews"));
        assert!(router.has_route("listTriggers"));
        assert!(router.has_route("listFunctions"));
        assert!(router.has_route("listProcedures"));
        assert!(router.has_route("getTableSchema"));
        assert!(router.has_route("readQuery"));
        assert!(router.has_route("explainQuery"));
        assert!(!router.has_route("writeQuery"));
        assert!(!router.has_route("createDatabase"));
        assert!(!router.has_route("dropDatabase"));
        assert!(!router.has_route("dropTable"));
    }

    #[tokio::test]
    async fn router_does_not_advertise_list_materialized_views() {
        let router = handler(false).tool_router;
        assert!(
            !router.has_route("listMaterializedViews"),
            "MySQL must not advertise listMaterializedViews"
        );
    }
}
