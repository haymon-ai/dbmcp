//! `SQLite` handler: composes a [`SqliteConnection`] with the MCP tool router.
//!
//! All pool ownership and pool initialization logic lives in the
//! [`SqliteConnection`]. This module exposes the MCP
//! `ServerHandler` surface and one thin delegator method that the
//! per-tool implementations call.

use dbmcp_privacy::{MaybeRedact, Redactor};
use dbmcp_server::{PoolFuture, Pools, Server, ToolHandler, ToolRouterExt, ToolSpec, server_info};
use dbmcp_sql::Connection;
use dbmcp_types::{Config, database, privacy};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::ServerConfig;
use rmcp::{ServerHandler, tool_handler};

use crate::connection::SqliteConnection;
use crate::tools::{ExplainQueryTool, ObjectsTool, ReadQueryTool, WriteQueryTool};

/// Backend-specific description for `SQLite`.
const DESCRIPTION: &str = "The privacy first MCP server for SQL databases.";

/// Backend-specific instructions for `SQLite` in read-write mode.
const INSTRUCTIONS: &str = include_str!("../assets/instructions/default.md");

/// Backend-specific instructions for `SQLite` in read-only mode.
const INSTRUCTIONS_READ_ONLY: &str = include_str!("../assets/instructions/read-only.md");

/// Declarative tool table: `(tool, pinned, read_only)`.
///
/// `SQLite` has no `database` field, so no tool is pinned.
const TOOLS: &[ToolSpec<SqliteHandler>] = &[
    ToolSpec::tool::<ObjectsTool>(false, false),
    ToolSpec::tool::<ReadQueryTool>(false, false),
    ToolSpec::tool::<ExplainQueryTool>(false, false),
    ToolSpec::tool::<WriteQueryTool>(false, true),
];

/// `SQLite` file-based database handler.
///
/// Composes one `SqliteConnection` (which owns the pool and
/// the pool initialization logic) with the per-backend MCP tool router.
#[derive(Clone)]
pub struct SqliteHandler {
    pub(crate) config: database::Config,
    pub(crate) connection: SqliteConnection,
    pub(crate) redactor: Option<Redactor>,
    /// PII settings with column rules; `None` without rules.
    pub(crate) policy: Option<privacy::Config>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for SqliteHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteHandler")
            .field("read_only", &self.config.read_only)
            .field("redact_pii", &self.redactor.is_some())
            .field("column_rules", &self.policy.is_some())
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl SqliteHandler {
    /// Creates a new `SQLite` handler.
    ///
    /// Constructs the `SqliteConnection` (which builds the
    /// lazy pool) and the MCP tool router. No file I/O happens here.
    /// # Errors
    ///
    /// Returns the [`dbmcp_privacy::RedactorInitError`] from
    /// [`dbmcp_privacy::Redactor::from_config`] (fail-closed startup).
    pub fn new(config: &Config) -> Result<Self, dbmcp_privacy::RedactorInitError> {
        let policy = config.privacy.clone().filter(|privacy| !privacy.rules.is_empty());
        Ok(Self {
            config: config.database.clone(),
            connection: SqliteConnection::new(&config.database, policy.is_some()),
            redactor: Redactor::from_config(config.privacy.as_ref())?,
            policy,
            tool_router: ToolRouter::from_specs(TOOLS, config.database.read_only, false),
        })
    }
}

impl From<SqliteHandler> for Server {
    /// Wraps a [`SqliteHandler`] in the type-erased MCP server.
    fn from(handler: SqliteHandler) -> Self {
        Self::new(handler)
    }
}

impl ToolHandler for SqliteHandler {
    fn redact_error(&self, message: &str) -> String {
        self.redactor.redact_message(message)
    }
}

impl Pools for SqliteHandler {
    fn ping(&self) -> PoolFuture<'_, bool> {
        Box::pin(async move { self.connection.ping().await.is_ok() })
    }

    fn shutdown(&self) -> PoolFuture<'_, ()> {
        Box::pin(async move { self.connection.shutdown().await })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SqliteHandler {
    fn get_info(&self) -> ServerConfig {
        let mut info = server_info();
        info.server_info.description = Some(DESCRIPTION.into());
        info.instructions = Some(if self.config.read_only {
            INSTRUCTIONS_READ_ONLY.into()
        } else {
            INSTRUCTIONS.into()
        });
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler(read_only: bool) -> SqliteHandler {
        SqliteHandler::new(&Config {
            transport: dbmcp_types::Transport::Stdio,
            database: database::Config {
                backend: database::Backend::Sqlite,
                name: Some(":memory:".into()),
                read_only,
                ..database::Config::default()
            },
            http: None,
            privacy: None,
            ssh: None,
        })
        .expect("handler builds in test")
    }

    fn tool_names(router: &ToolRouter<SqliteHandler>) -> Vec<String> {
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
        assert_eq!(
            tool_names(&handler(false).tool_router),
            ["explain", "objects", "read", "write"]
        );
    }

    #[tokio::test]
    async fn router_hides_write_in_read_only_mode() {
        assert_eq!(tool_names(&handler(true).tool_router), ["explain", "objects", "read"]);
    }

    #[tokio::test]
    async fn instructions_match_read_only_mode() {
        let read_write = handler(false).get_info().instructions.expect("instructions present");
        assert!(read_write.contains("`write`"));
        let read_only = handler(true).get_info().instructions.expect("instructions present");
        assert!(!read_only.contains("`write`"));
    }

    #[tokio::test]
    async fn objects_annotations() {
        let router = handler(false).tool_router;
        let tool = router.get("objects").expect("objects registered");

        let annotations = tool.annotations.as_ref().expect("annotations present");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }
}
