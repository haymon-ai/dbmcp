//! `SQLite` handler: composes a [`SqliteConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`SqliteConnection`]. This module exposes the MCP
//! `ServerHandler` surface and one thin delegator method that the
//! per-tool implementations call.

use dbmcp_config::{Config, DatabaseConfig};
use dbmcp_pii::Redactor;
use dbmcp_server::{Server, ToolRouterExt, ToolSpec, server_info};
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};

use crate::connection::SqliteConnection;
use crate::tools::{
    DropTableTool, ExplainQueryTool, ListTablesTool, ListTriggersTool, ListViewsTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for `SQLite`.
const DESCRIPTION: &str = "Database MCP Server for SQLite";

/// Backend-specific instructions for `SQLite` in read-write mode.
const INSTRUCTIONS: &str = include_str!("../assets/instructions/default.md");

/// Backend-specific instructions for `SQLite` in read-only mode.
const INSTRUCTIONS_READ_ONLY: &str = include_str!("../assets/instructions/read-only.md");

/// Declarative tool table: `(tool, read_only, pinned)`.
///
/// `SQLite` has no cross-database tools, so every entry is `pinned = false`.
const TOOLS: &[ToolSpec<SqliteHandler>] = &[
    ToolSpec::async_tool::<ListTablesTool>(false, false),
    ToolSpec::async_tool::<ListViewsTool>(false, false),
    ToolSpec::async_tool::<ListTriggersTool>(false, false),
    ToolSpec::async_tool::<ReadQueryTool>(false, false),
    ToolSpec::async_tool::<ExplainQueryTool>(false, false),
    ToolSpec::async_tool::<WriteQueryTool>(true, false),
    ToolSpec::async_tool::<DropTableTool>(true, false),
];

/// `SQLite` file-based database handler.
///
/// Composes one [`SqliteConnection`] (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct SqliteHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) connection: SqliteConnection,
    pub(crate) redactor: Option<Redactor>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for SqliteHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteHandler")
            .field("read_only", &self.config.read_only)
            .field("redact_pii", &self.redactor.is_some())
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
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.database.clone(),
            connection: SqliteConnection::new(&config.database),
            redactor: Redactor::from_config(&config.pii),
            tool_router: ToolRouter::from_specs(TOOLS, config.database.read_only, false),
        }
    }
}

impl From<SqliteHandler> for Server {
    /// Wraps a [`SqliteHandler`] in the type-erased MCP server.
    fn from(handler: SqliteHandler) -> Self {
        Self::new(handler)
    }
}

impl ServerHandler for SqliteHandler {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(if self.config.read_only {
            INSTRUCTIONS_READ_ONLY.into()
        } else {
            INSTRUCTIONS.into()
        });
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

    fn handler(read_only: bool) -> SqliteHandler {
        SqliteHandler::new(&Config {
            database: DatabaseConfig {
                backend: DatabaseBackend::Sqlite,
                name: Some(":memory:".into()),
                read_only,
                ..DatabaseConfig::default()
            },
            http: None,
            pii: dbmcp_config::PiiConfig::default(),
        })
    }

    #[tokio::test]
    async fn router_exposes_all_seven_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "listTables",
            "listViews",
            "listTriggers",
            "dropTable",
            "readQuery",
            "writeQuery",
            "explainQuery",
        ] {
            assert!(router.has_route(name), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn router_excludes_get_table_schema() {
        // Spec 046 US4: `getTableSchema` is retired on SQLite. Both read-only and
        // read-write catalogues must no longer advertise it.
        for read_only in [false, true] {
            let router = handler(read_only).tool_router;
            assert!(
                !router.has_route("getTableSchema"),
                "getTableSchema must be absent (read_only={read_only})"
            );
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
        assert!(router.has_route("readQuery"));
        assert!(router.has_route("explainQuery"));
        assert!(!router.has_route("writeQuery"));
        assert!(!router.has_route("dropTable"));
    }

    #[tokio::test]
    async fn instructions_match_read_only_mode() {
        let read_write = handler(false).get_info().instructions.expect("instructions present");
        assert!(
            read_write.contains("writeQuery"),
            "read-write instructions mention writeQuery"
        );

        let read_only = handler(true).get_info().instructions.expect("instructions present");
        for tool in ["writeQuery", "dropTable"] {
            assert!(
                !read_only.contains(tool),
                "read-only instructions must not mention {tool}"
            );
        }
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
