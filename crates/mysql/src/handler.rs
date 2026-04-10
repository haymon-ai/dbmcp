//! MySQL/MariaDB handler: connection pool, MCP tool router, and `ServerHandler` impl.
//!
//! Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`] and creates
//! a lazy connection pool via [`MySqlPoolOptions::connect_lazy_with`].
//! No network I/O happens until the first tool invocation.

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_server::server_info;
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};
use serde_json::Value;
use sqlx::Executor;
use sqlx::MySqlPool;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx_to_json::RowExt;
use tracing::info;

use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListDatabasesTool,
    ListTablesTool, ReadQueryTool, WriteQueryTool,
};

/// Backend-specific description for MySQL/MariaDB.
const DESCRIPTION: &str = "Database MCP Server for MySQL and MariaDB";

/// Backend-specific instructions for MySQL/MariaDB.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_databases` to discover available databases.
2. Call `list_tables` with a `database_name` to see its tables.
3. Call `get_table_schema` with `database_name` and `table_name` to inspect columns, types, and foreign keys before writing queries.
4. Use `read_query` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
5. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `create_database` to create a new database.
7. Use `drop_database` to drop an existing database.

## Constraints

- The `write_query`, `create_database`, and `drop_database` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// MySQL/MariaDB database handler.
///
/// The connection pool is created with [`MySqlPoolOptions::connect_lazy_with`],
/// which defers all network I/O until the first query. Connection errors
/// surface as tool-level errors returned to the MCP client.
#[derive(Clone)]
pub struct MysqlHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: MySqlPool,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MysqlHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlHandler")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl MysqlHandler {
    /// Creates a new `MySQL` handler with a lazy connection pool.
    ///
    /// Does **not** establish a database connection. The pool connects
    /// on-demand when the first query is executed. The MCP tool router
    /// is built once here and reused for every request.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        let pool = pool_options(config).connect_lazy_with(connect_options(config));
        info!(
            "MySQL lazy connection pool created (max size: {})",
            config.max_pool_size
        );
        Self {
            config: config.clone(),
            pool,
            tool_router: build_tool_router(config.read_only),
        }
    }

    /// Wraps `name` in backticks for safe use in `MySQL` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '`')
    }

    /// Wraps a value in single quotes for use as a SQL string literal.
    ///
    /// Escapes internal single quotes by doubling them.
    pub(crate) fn quote_string(value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
    }

    /// Executes raw SQL and converts rows to JSON maps.
    ///
    /// Uses the text protocol via `Executor::fetch_all(&str)` instead of prepared
    /// statements, because `MySQL` 9+ doesn't support SHOW commands as prepared
    /// statements, and the text protocol returns all values as strings.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub(crate) async fn query_to_json(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        // Validate before entering the timeout scope so validation errors
        // are not confused with timeouts.
        if let Some(db) = database {
            validate_identifier(db)?;
        }

        let pool = self.pool.clone();
        let db = database.map(String::from);
        let sql_owned = sql.to_string();

        // The timeout wraps the entire acquire → USE → fetch sequence
        // because from the caller's perspective, this is one operation.
        execute_with_timeout(self.config.query_timeout, sql, async move {
            let mut conn = pool.acquire().await?;

            if let Some(db) = &db {
                let use_sql = format!("USE {}", Self::quote_identifier(db));
                conn.execute(use_sql.as_str()).await?;
            }

            let rows: Vec<MySqlRow> = conn.fetch_all(sql_owned.as_str()).await?;
            Ok::<_, sqlx::Error>(Value::Array(rows.iter().map(RowExt::to_json).collect()))
        })
        .await
    }
}

/// Builds [`MySqlPoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> MySqlPoolOptions {
    let mut opts = MySqlPoolOptions::new()
        .max_connections(config.max_pool_size)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> MySqlConnectOptions {
    let mut opts = MySqlConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.user);

    if let Some(ref password) = config.password {
        opts = opts.password(password);
    }
    if let Some(ref name) = config.name
        && !name.is_empty()
    {
        opts = opts.database(name);
    }
    if let Some(ref charset) = config.charset {
        opts = opts.charset(charset);
    }

    if config.ssl {
        opts = if config.ssl_verify_cert {
            opts.ssl_mode(MySqlSslMode::VerifyCa)
        } else {
            opts.ssl_mode(MySqlSslMode::Required)
        };
        if let Some(ref ca) = config.ssl_ca {
            opts = opts.ssl_ca(ca);
        }
        if let Some(ref cert) = config.ssl_cert {
            opts = opts.ssl_client_cert(cert);
        }
        if let Some(ref key) = config.ssl_key {
            opts = opts.ssl_client_key(key);
        }
    }

    opts
}

/// Builds the tool router, including write tools only when not in read-only mode.
fn build_tool_router(read_only: bool) -> ToolRouter<MysqlHandler> {
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
    use database_mcp_config::DatabaseBackend;

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

    #[test]
    fn pool_options_applies_defaults() {
        let config = base_config();
        let opts = pool_options(&config);

        assert_eq!(opts.get_max_connections(), config.max_pool_size);
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

        // sqlx defaults acquire_timeout to 30s when not overridden
        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn try_from_basic_config() {
        let config = base_config();
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "db.example.com");
        assert_eq!(opts.get_port(), 3307);
        assert_eq!(opts.get_username(), "admin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_charset() {
        let config = DatabaseConfig {
            charset: Some("utf8mb4".into()),
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_charset(), "utf8mb4");
    }

    #[test]
    fn try_from_with_ssl_required() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), MySqlSslMode::Required),
            "expected Required, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_with_ssl_verify_ca() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: true,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), MySqlSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        // Should not panic — password is simply omitted
        assert_eq!(opts.get_host(), "db.example.com");
    }

    #[test]
    fn try_from_without_database_name() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_database(), None);
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let config = base_config();
        let handler = MysqlHandler::new(&config);
        assert!(handler.config.read_only);
        // Pool exists but has no active connections (lazy).
        assert_eq!(handler.pool.size(), 0);
    }

    #[tokio::test]
    async fn router_exposes_all_nine_tools_in_read_write_mode() {
        let router = handler(false).tool_router;
        for name in [
            "list_databases",
            "list_tables",
            "get_table_schema",
            "read_query",
            "explain_query",
            "create_database",
            "drop_database",
            "drop_table",
            "write_query",
        ] {
            assert!(router.has_route(name), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn router_hides_write_tools_in_read_only_mode() {
        let router = handler(true).tool_router;
        assert!(router.has_route("list_databases"));
        assert!(router.has_route("list_tables"));
        assert!(router.has_route("get_table_schema"));
        assert!(router.has_route("read_query"));
        assert!(router.has_route("explain_query"));
        assert!(!router.has_route("write_query"));
        assert!(!router.has_route("create_database"));
        assert!(!router.has_route("drop_database"));
        assert!(!router.has_route("drop_table"));
    }
}
