//! `SQLite` handler: connection pool, MCP tool router, and `ServerHandler` impl.
//!
//! Uses [`SqlitePoolOptions::connect_lazy_with`] so no file I/O happens
//! until the first tool invocation.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::server_info;
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::tools::{
    DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListTablesTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for `SQLite`.
const DESCRIPTION: &str = "Database MCP Server for SQLite";

/// Backend-specific instructions for `SQLite`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_tables` to discover tables in the connected database.
2. Call `get_table_schema` with a `table_name` to inspect columns, types, and foreign keys before writing queries.
3. Use `read_query` for read-only SQL (SELECT, EXPLAIN).
4. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

## Constraints

- The `write_query` tool is hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// `SQLite` file-based database handler.
///
/// The connection pool is created with [`SqlitePoolOptions::connect_lazy_with`],
/// which defers all file I/O until the first query. Connection errors
/// surface as tool-level errors returned to the MCP client.
#[derive(Clone)]
pub struct SqliteHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: SqlitePool,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for SqliteHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteHandler")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl SqliteHandler {
    /// Creates a new `SQLite` handler with a lazy connection pool.
    ///
    /// Does **not** open the database file. The pool connects on-demand
    /// when the first query is executed. The MCP tool router is built once
    /// here and reused for every request.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        Self {
            config: config.clone(),
            pool: pool_options(config).connect_lazy_with(connect_options(config)),
            tool_router: build_tool_router(config.read_only),
        }
    }

    /// Wraps `name` in double quotes for safe use in `SQLite` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }
}

/// Builds [`SqlitePoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> SqlitePoolOptions {
    let mut opts = SqlitePoolOptions::new()
        .max_connections(1) // SQLite is a single-writer
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`SqliteConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> SqliteConnectOptions {
    SqliteConnectOptions::new().filename(config.name.as_deref().unwrap_or_default())
}

/// Builds the tool router, including write tools only when not in read-only mode.
fn build_tool_router(read_only: bool) -> ToolRouter<SqliteHandler> {
    let mut router = ToolRouter::new()
        .with_async_tool::<ListTablesTool>()
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

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        }
    }

    fn handler(read_only: bool) -> SqliteHandler {
        SqliteHandler::new(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some(":memory:".into()),
            read_only,
            ..DatabaseConfig::default()
        })
    }

    #[test]
    fn pool_options_applies_defaults() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), 1, "SQLite must be single-writer");
        assert_eq!(opts.get_min_connections(), DatabaseConfig::DEFAULT_MIN_CONNECTIONS);
        assert_eq!(
            opts.get_idle_timeout(),
            Some(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        );
        assert_eq!(
            opts.get_max_lifetime(),
            Some(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS))
        );
    }

    #[test]
    fn pool_options_applies_connection_timeout() {
        let config = DatabaseConfig {
            connection_timeout: Some(7),
            ..base_config()
        };
        let opts = pool_options(&config);

        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(7));
    }

    #[test]
    fn pool_options_without_connection_timeout_uses_sqlx_default() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn pool_options_ignores_max_pool_size() {
        let config = DatabaseConfig {
            max_pool_size: 20,
            ..base_config()
        };
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), 1, "SQLite must always be single-writer");
    }

    #[test]
    fn try_from_sets_filename() {
        let opts = connect_options(&base_config());

        assert_eq!(opts.get_filename().to_str().expect("valid path"), "test.db");
    }

    #[test]
    fn try_from_empty_name_defaults() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        // Empty string filename — validated elsewhere by Config::validate()
        assert_eq!(opts.get_filename().to_str().expect("valid path"), "");
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let config = base_config();
        let handler = SqliteHandler::new(&config);
        // Pool exists but has no active connections (lazy).
        assert_eq!(handler.pool.size(), 0);
    }

    #[tokio::test]
    async fn router_exposes_all_six_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "list_tables",
            "get_table_schema",
            "drop_table",
            "read_query",
            "write_query",
            "explain_query",
        ] {
            assert!(router.has_route(name), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn router_hides_write_tools_in_read_only_mode() {
        let router = handler(true).tool_router;
        assert!(router.has_route("list_tables"));
        assert!(router.has_route("get_table_schema"));
        assert!(router.has_route("read_query"));
        assert!(router.has_route("explain_query"));
        assert!(!router.has_route("write_query"));
        assert!(!router.has_route("drop_table"));
    }

    #[tokio::test]
    async fn list_tables_metadata_matches_macro_parity() {
        let router = handler(false).tool_router;
        let tool = router.get("list_tables").expect("list_tables registered");

        assert_eq!(tool.name, "list_tables");
        assert_eq!(
            tool.description.as_deref(),
            Some("List all tables in the connected `SQLite` database.")
        );

        let annotations = tool.annotations.as_ref().expect("annotations present");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }
}
