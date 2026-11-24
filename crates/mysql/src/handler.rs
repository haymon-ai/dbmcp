//! MySQL/MariaDB handler: composes a [`MysqlConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`MysqlConnection`]. This module exposes the MCP `ServerHandler`
//! surface and a small set of thin delegators that per-tool
//! implementations call.

use dbmcp_privacy::{MaybeRedact, Redactor};
use dbmcp_server::{PoolFuture, Pools, Server, ToolHandler, ToolRouterExt, ToolSpec, server_info};
use dbmcp_sql::Connection;
use dbmcp_types::{Config, database, privacy};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::ServerConfig;
use rmcp::{ServerHandler, tool_handler};

use crate::connection::MysqlConnection;
use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, ListDatabasesTool, ListFunctionsTool,
    ListProceduresTool, ListTablesTool, ListTriggersTool, ListViewsTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for MySQL/MariaDB.
const DESCRIPTION: &str = "The privacy first MCP server for SQL databases.";

/// Backend-specific instructions for MySQL/MariaDB in read-write mode.
const INSTRUCTIONS: &str = include_str!("../assets/instructions/default.md");

/// Backend-specific instructions for MySQL/MariaDB in read-only mode.
const INSTRUCTIONS_READ_ONLY: &str = include_str!("../assets/instructions/read-only.md");

/// Backend-specific instructions when a database name is pinned.
const INSTRUCTIONS_PINNED: &str = include_str!("../assets/instructions/default.pinned.md");

/// Backend-specific instructions for read-only mode with a pinned database.
const INSTRUCTIONS_READ_ONLY_PINNED: &str = include_str!("../assets/instructions/read-only.pinned.md");

/// Declarative tool table: `(tool, pinned, read_only)`.
///
/// Per-database tools register as `Tool<true>` when pinned, else `Tool<false>`.
/// Cross-database tools are hidden when pinned.
const TOOLS: &[ToolSpec<MysqlHandler>] = &[
    ToolSpec::tool::<ListDatabasesTool>(false, false),
    ToolSpec::tool::<ListTablesTool<true>>(true, false),
    ToolSpec::tool::<ListTablesTool<false>>(false, false),
    ToolSpec::tool::<ListViewsTool<true>>(true, false),
    ToolSpec::tool::<ListViewsTool<false>>(false, false),
    ToolSpec::tool::<ListTriggersTool<true>>(true, false),
    ToolSpec::tool::<ListTriggersTool<false>>(false, false),
    ToolSpec::tool::<ListFunctionsTool<true>>(true, false),
    ToolSpec::tool::<ListFunctionsTool<false>>(false, false),
    ToolSpec::tool::<ListProceduresTool<true>>(true, false),
    ToolSpec::tool::<ListProceduresTool<false>>(false, false),
    ToolSpec::tool::<ReadQueryTool<true>>(true, false),
    ToolSpec::tool::<ReadQueryTool<false>>(false, false),
    ToolSpec::tool::<ExplainQueryTool<true>>(true, false),
    ToolSpec::tool::<ExplainQueryTool<false>>(false, false),
    ToolSpec::tool::<CreateDatabaseTool>(false, true),
    ToolSpec::tool::<DropDatabaseTool>(false, true),
    ToolSpec::tool::<DropTableTool<true>>(true, true),
    ToolSpec::tool::<DropTableTool<false>>(false, true),
    ToolSpec::tool::<WriteQueryTool<true>>(true, true),
    ToolSpec::tool::<WriteQueryTool<false>>(false, true),
];

/// MySQL/MariaDB database handler.
///
/// Composes one `MysqlConnection` (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct MysqlHandler {
    pub(crate) config: database::Config,
    pub(crate) connection: MysqlConnection,
    pub(crate) redactor: Option<Redactor>,
    /// PII settings with column rules; `None` without rules.
    pub(crate) policy: Option<privacy::Config>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MysqlHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlHandler")
            .field("read_only", &self.config.read_only)
            .field("redact_pii", &self.redactor.is_some())
            .field("column_rules", &self.policy.is_some())
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl MysqlHandler {
    /// Creates a new `MySQL` handler.
    ///
    /// Constructs the `MysqlConnection` (which builds the
    /// lazy pool) and the MCP tool router. No network I/O happens here.
    /// # Errors
    ///
    /// Returns the [`dbmcp_privacy::RedactorInitError`] from
    /// [`dbmcp_privacy::Redactor::from_config`] (fail-closed startup).
    pub fn new(config: &Config) -> Result<Self, dbmcp_privacy::RedactorInitError> {
        let policy = config.privacy.clone().filter(|privacy| !privacy.rules.is_empty());
        Ok(Self {
            config: config.database.clone(),
            connection: MysqlConnection::new(&config.database, policy.is_some()),
            redactor: Redactor::from_config(config.privacy.as_ref())?,
            policy,
            tool_router: ToolRouter::from_specs(TOOLS, config.database.read_only, config.database.name.is_some()),
        })
    }
}

impl From<MysqlHandler> for Server {
    /// Wraps a [`MysqlHandler`] in the type-erased MCP server.
    fn from(handler: MysqlHandler) -> Self {
        Self::new(handler)
    }
}

impl ToolHandler for MysqlHandler {
    fn redact_error(&self, message: &str) -> String {
        self.redactor.redact_message(message)
    }
}

impl Pools for MysqlHandler {
    fn ping(&self) -> PoolFuture<'_, bool> {
        Box::pin(async move { self.connection.ping().await.is_ok() })
    }

    fn shutdown(&self) -> PoolFuture<'_, ()> {
        Box::pin(async move { self.connection.shutdown().await })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MysqlHandler {
    fn get_info(&self) -> ServerConfig {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> database::Config {
        database::Config {
            backend: database::Backend::Mysql,
            host: "db.example.com".into(),
            port: 3307,
            user: "admin".into(),
            password: Some("s3cret".into()),
            name: None,
            ..database::Config::default()
        }
    }

    fn handler(read_only: bool) -> MysqlHandler {
        MysqlHandler::new(&Config {
            transport: dbmcp_types::Transport::Stdio,
            database: database::Config {
                read_only,
                ..base_config()
            },
            http: None,
            privacy: None,
            ssh: None,
        })
        .expect("handler builds in test")
    }

    /// Handler whose config pins a specific database name.
    fn pinned_handler(read_only: bool) -> MysqlHandler {
        MysqlHandler::new(&Config {
            transport: dbmcp_types::Transport::Stdio,
            database: database::Config {
                read_only,
                name: Some("mydb".into()),
                ..base_config()
            },
            http: None,
            privacy: None,
            ssh: None,
        })
        .expect("handler builds in test")
    }

    #[test]
    fn policy_exists_only_with_column_rules() {
        let with = |rules| {
            MysqlHandler::new(&Config {
                transport: dbmcp_types::Transport::Stdio,
                database: base_config(),
                http: None,
                privacy: Some(privacy::Config {
                    operator: dbmcp_types::privacy::Operator::Mask,
                    rules,
                    ..privacy::Config::default()
                }),
                ssh: None,
            })
            .expect("handler builds in test")
        };
        assert!(handler(false).policy.is_none());
        assert!(with(Vec::new()).policy.is_none());
        let rule = dbmcp_types::privacy::ColumnRule {
            name: "emails".into(),
            item: 0,
            key: ["def", "app", "users", "email"].map(String::from),
            entity: dbmcp_types::privacy::Entity::EmailAddress,
            operator: dbmcp_types::privacy::Operator::Mask,
        };
        assert!(with(vec![rule]).policy.is_some());
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
