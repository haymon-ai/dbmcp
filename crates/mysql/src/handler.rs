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
use crate::tools::{ExplainTool, ObjectsTool, ReadTool, WriteTool};

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
    ToolSpec::tool::<ObjectsTool<true>>(true, false),
    ToolSpec::tool::<ObjectsTool<false>>(false, false),
    ToolSpec::tool::<ReadTool<true>>(true, false),
    ToolSpec::tool::<ReadTool<false>>(false, false),
    ToolSpec::tool::<ExplainTool<true>>(true, false),
    ToolSpec::tool::<ExplainTool<false>>(false, false),
    ToolSpec::tool::<WriteTool<true>>(true, true),
    ToolSpec::tool::<WriteTool<false>>(false, true),
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

    fn tool_names(router: &ToolRouter<MysqlHandler>) -> Vec<String> {
        let mut names: Vec<String> = router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn router_exposes_four_tools_in_read_write_mode() {
        for router in [handler(false).tool_router, pinned_handler(false).tool_router] {
            assert_eq!(tool_names(&router), ["explain", "objects", "read", "write"]);
        }
    }

    #[tokio::test]
    async fn router_hides_write_in_read_only_mode() {
        for router in [handler(true).tool_router, pinned_handler(true).tool_router] {
            assert_eq!(tool_names(&router), ["explain", "objects", "read"]);
        }
    }

    #[tokio::test]
    async fn objects_is_read_only_and_write_destructive() {
        let router = handler(false).tool_router;
        let objects = router.get("objects").expect("objects registered");
        let annotations = objects.annotations.as_ref().expect("annotations present");
        assert_eq!(annotations.read_only_hint, Some(true));
        let write = router.get("write").expect("write registered");
        let annotations = write.annotations.as_ref().expect("annotations present");
        assert_eq!(annotations.destructive_hint, Some(true));
    }

    #[tokio::test]
    async fn pinned_objects_schema_drops_database() {
        let pinned = pinned_handler(true).tool_router;
        let unpinned = handler(true).tool_router;
        let has_database = |router: &ToolRouter<MysqlHandler>| {
            router.get("objects").expect("objects registered").input_schema["properties"]
                .as_object()
                .expect("properties")
                .contains_key("database")
        };
        assert!(has_database(&unpinned));
        assert!(!has_database(&pinned));
    }

    #[tokio::test]
    async fn instructions_name_only_registered_tools() {
        for (read_only, instructions) in [
            (false, handler(false).get_info().instructions),
            (true, handler(true).get_info().instructions),
            (false, pinned_handler(false).get_info().instructions),
            (true, pinned_handler(true).get_info().instructions),
        ] {
            let instructions = instructions.expect("instructions present");
            assert!(instructions.contains("`objects`"));
            assert_eq!(instructions.contains("`write`"), !read_only);
            for old in [
                "listTables",
                "readQuery",
                "writeQuery",
                "explainQuery",
                "dropTable",
                "createDatabase",
            ] {
                assert!(!instructions.contains(old), "instructions must not mention {old}");
            }
        }
    }
}
