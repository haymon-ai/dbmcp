//! MySQL/MariaDB handler: composes a [`MysqlConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`MysqlConnection`]. This module exposes the MCP `ServerHandler`
//! surface and a small set of thin delegators that per-tool
//! implementations call.

use dbmcp_config::{Config, DatabaseConfig};
use dbmcp_pii::Redactor;
use dbmcp_server::{Server, ToolRouterExt, ToolSpec, server_info};
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};

use crate::connection::MysqlConnection;
use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, ListDatabasesTool, ListFunctionsTool,
    ListProceduresTool, ListTablesTool, ListTriggersTool, ListViewsTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for MySQL/MariaDB.
const DESCRIPTION: &str = "Database MCP Server for MySQL and MariaDB";

/// Backend-specific instructions for MySQL/MariaDB in read-write mode.
const INSTRUCTIONS: &str = include_str!("../assets/instructions/default.md");

/// Backend-specific instructions for MySQL/MariaDB in read-only mode.
const INSTRUCTIONS_READ_ONLY: &str = include_str!("../assets/instructions/read-only.md");

/// Backend-specific instructions when a database name is pinned.
const INSTRUCTIONS_PINNED: &str = include_str!("../assets/instructions/default.pinned.md");

/// Backend-specific instructions for read-only mode with a pinned database.
const INSTRUCTIONS_READ_ONLY_PINNED: &str = include_str!("../assets/instructions/read-only.pinned.md");

/// Declarative tool table: `(tool, read_only, pinned)`.
const TOOLS: &[ToolSpec<MysqlHandler>] = &[
    ToolSpec::async_tool::<ListDatabasesTool>(false, true),
    ToolSpec::async_tool::<ListTablesTool>(false, false),
    ToolSpec::async_tool::<ListViewsTool>(false, false),
    ToolSpec::async_tool::<ListTriggersTool>(false, false),
    ToolSpec::async_tool::<ListFunctionsTool>(false, false),
    ToolSpec::async_tool::<ListProceduresTool>(false, false),
    ToolSpec::async_tool::<ReadQueryTool>(false, false),
    ToolSpec::async_tool::<ExplainQueryTool>(false, false),
    ToolSpec::async_tool::<CreateDatabaseTool>(true, true),
    ToolSpec::async_tool::<DropDatabaseTool>(true, true),
    ToolSpec::async_tool::<DropTableTool>(true, false),
    ToolSpec::async_tool::<WriteQueryTool>(true, false),
];

/// MySQL/MariaDB database handler.
///
/// Composes one [`MysqlConnection`] (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct MysqlHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) connection: MysqlConnection,
    pub(crate) redactor: Option<Redactor>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MysqlHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlHandler")
            .field("read_only", &self.config.read_only)
            .field("redact_pii", &self.redactor.is_some())
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
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.database.clone(),
            connection: MysqlConnection::new(&config.database),
            redactor: Redactor::from_config(&config.pii),
            tool_router: ToolRouter::from_specs(TOOLS, config.database.read_only, config.database.name.is_some()),
        }
    }
}

impl From<MysqlHandler> for Server {
    /// Wraps a [`MysqlHandler`] in the type-erased MCP server.
    fn from(handler: MysqlHandler) -> Self {
        Self::new(handler)
    }
}

impl ServerHandler for MysqlHandler {
    fn get_info(&self) -> ServerInfo {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(
            match (self.config.read_only, self.config.name.is_some()) {
                (false, false) => INSTRUCTIONS,
                (true, false) => INSTRUCTIONS_READ_ONLY,
                (false, true) => INSTRUCTIONS_PINNED,
                (true, true) => INSTRUCTIONS_READ_ONLY_PINNED,
            }
            .into(),
        );
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
            name: None,
            ..DatabaseConfig::default()
        }
    }

    fn handler(read_only: bool) -> MysqlHandler {
        MysqlHandler::new(&Config {
            database: DatabaseConfig {
                read_only,
                ..base_config()
            },
            http: None,
            pii: dbmcp_config::PiiConfig::default(),
        })
    }

    /// Handler whose config pins a specific database name.
    fn pinned_handler(read_only: bool) -> MysqlHandler {
        MysqlHandler::new(&Config {
            database: DatabaseConfig {
                read_only,
                name: Some("mydb".into()),
                ..base_config()
            },
            http: None,
            pii: dbmcp_config::PiiConfig::default(),
        })
    }

    #[tokio::test]
    async fn router_exposes_all_twelve_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "listDatabases",
            "listTables",
            "listViews",
            "listTriggers",
            "listFunctions",
            "listProcedures",
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
        assert!(router.has_route("readQuery"));
        assert!(router.has_route("explainQuery"));
        assert!(!router.has_route("writeQuery"));
        assert!(!router.has_route("createDatabase"));
        assert!(!router.has_route("dropDatabase"));
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
        for tool in ["writeQuery", "createDatabase", "dropDatabase", "dropTable"] {
            assert!(
                !read_only.contains(tool),
                "read-only instructions must not mention {tool}"
            );
        }
    }

    #[tokio::test]
    async fn router_does_not_advertise_list_materialized_views() {
        let router = handler(false).tool_router;
        assert!(
            !router.has_route("listMaterializedViews"),
            "MySQL must not advertise listMaterializedViews"
        );
    }

    #[tokio::test]
    async fn router_does_not_expose_get_table_schema() {
        let rw = handler(false).tool_router;
        let ro = handler(true).tool_router;
        assert!(
            !rw.has_route("getTableSchema"),
            "read-write router must not expose getTableSchema"
        );
        assert!(
            !ro.has_route("getTableSchema"),
            "read-only router must not expose getTableSchema"
        );
    }

    #[tokio::test]
    async fn router_hides_cross_database_tools_when_name_pinned() {
        let router = pinned_handler(false).tool_router;
        for present in ["listTables", "readQuery", "explainQuery", "dropTable", "writeQuery"] {
            assert!(router.has_route(present), "missing tool: {present}");
        }
        for absent in ["listDatabases", "createDatabase", "dropDatabase"] {
            assert!(!router.has_route(absent), "pinned router must not expose {absent}");
        }
    }

    #[tokio::test]
    async fn router_hides_list_databases_when_name_pinned_read_only() {
        let router = pinned_handler(true).tool_router;
        assert!(!router.has_route("listDatabases"));
        assert!(!router.has_route("createDatabase"));
        assert!(!router.has_route("dropDatabase"));
        assert!(router.has_route("listTables"));
        assert!(router.has_route("readQuery"));
    }

    #[tokio::test]
    async fn instructions_match_pinned_mode() {
        for read_only in [false, true] {
            let instructions = pinned_handler(read_only)
                .get_info()
                .instructions
                .expect("instructions present");
            for tool in ["listDatabases", "createDatabase", "dropDatabase"] {
                assert!(
                    !instructions.contains(tool),
                    "pinned instructions must not mention {tool} (read_only={read_only})"
                );
            }
        }
    }
}
