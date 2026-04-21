//! `SQLite` handler: composes a [`SqliteConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`SqliteConnection`]. This module exposes the MCP
//! `ServerHandler` surface and one thin delegator method that the
//! per-tool implementations call.

use database_mcp_config::DatabaseConfig;
use database_mcp_server::{Server, server_info};
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};

use crate::connection::SqliteConnection;
use crate::tools::{
    DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListTablesTool, ListTriggersTool, ListViewsTool,
    ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for `SQLite`.
const DESCRIPTION: &str = "Database MCP Server for SQLite";

/// Backend-specific instructions for `SQLite`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `listTables` to discover tables in the connected database.
2. Call `listViews` to discover views in the connected database.
3. Call `listTriggers` to discover triggers in the connected database.
4. Call `getTableSchema` with a `table` to inspect columns, types, and foreign keys before writing queries.
5. Use `readQuery` for read-only SQL (SELECT).
6. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
7. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
8. Use `dropTable` to remove a table from the database.

## Constraints

- The `writeQuery` and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// `SQLite` file-based database handler.
///
/// Composes one [`SqliteConnection`] (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct SqliteHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) connection: SqliteConnection,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for SqliteHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteHandler")
            .field("read_only", &self.config.read_only)
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl SqliteHandler {
    /// Creates a new `SQLite` handler.
    ///
    /// Constructs the [`SqliteConnection`] (which builds the
    /// lazy pool) and the MCP tool router. No file I/O happens here.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        Self {
            config: config.clone(),
            connection: SqliteConnection::new(config),
            tool_router: build_tool_router(config.read_only),
        }
    }
}

impl From<SqliteHandler> for Server {
    /// Wraps a [`SqliteHandler`] in the type-erased MCP server.
    fn from(handler: SqliteHandler) -> Self {
        Self::new(handler)
    }
}

/// Builds the tool router, including write tools only when not in read-only mode.
fn build_tool_router(read_only: bool) -> ToolRouter<SqliteHandler> {
    let mut router = ToolRouter::new()
        .with_async_tool::<ListTablesTool>()
        .with_async_tool::<ListViewsTool>()
        .with_async_tool::<ListTriggersTool>()
        .with_async_tool::<GetTableSchemaTool>()
        .with_async_tool::<ReadQueryTool>()
        .with_async_tool::<ExplainQueryTool>();

    if !read_only {
        router = router
            .with_async_tool::<WriteQueryTool>()
            .with_async_tool::<DropTableTool>();
    }
    router
}

impl ServerHandler for SqliteHandler {
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

    fn handler(read_only: bool) -> SqliteHandler {
        SqliteHandler::new(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some(":memory:".into()),
            read_only,
            ..DatabaseConfig::default()
        })
    }

    #[tokio::test]
    async fn router_exposes_all_eight_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "listTables",
            "listViews",
            "listTriggers",
            "getTableSchema",
            "dropTable",
            "readQuery",
            "writeQuery",
            "explainQuery",
        ] {
            assert!(router.has_route(name), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn router_does_not_advertise_backend_specific_tools() {
        let router = handler(false).tool_router;
        for absent in [
            "listDatabases",
            "listFunctions",
            "listProcedures",
            "listMaterializedViews",
            "createDatabase",
            "dropDatabase",
        ] {
            assert!(!router.has_route(absent), "SQLite must not advertise {absent}");
        }
    }

    #[tokio::test]
    async fn router_hides_write_tools_in_read_only_mode() {
        let router = handler(true).tool_router;
        assert!(router.has_route("listTables"));
        assert!(router.has_route("listViews"));
        assert!(router.has_route("listTriggers"));
        assert!(router.has_route("getTableSchema"));
        assert!(router.has_route("readQuery"));
        assert!(router.has_route("explainQuery"));
        assert!(!router.has_route("writeQuery"));
        assert!(!router.has_route("dropTable"));
    }

    #[tokio::test]
    async fn list_tables_annotations() {
        let router = handler(false).tool_router;
        let tool = router.get("listTables").expect("listTables registered");

        let annotations = tool.annotations.as_ref().expect("annotations present");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }
}
