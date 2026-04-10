//! `PostgreSQL` handler: connection pool cache, MCP tool router, and `ServerHandler` impl.
//!
//! Creates a lazy default pool via [`PgPoolOptions::connect_lazy_with`].
//! Non-default database pools are created on demand and cached in a
//! moka [`Cache`].

use std::time::Duration;

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use database_mcp_server::server_info;
use database_mcp_sql::identifier::validate_identifier;
use moka::future::Cache;
use rmcp::RoleServer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, ServerHandler};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use tracing::info;

use crate::tools::{
    CreateDatabaseTool, DropDatabaseTool, DropTableTool, ExplainQueryTool, GetTableSchemaTool, ListDatabasesTool,
    ListTablesTool, ReadQueryTool, WriteQueryTool,
};

/// Maximum number of database connection pools to cache (including the default).
const POOL_CACHE_CAPACITY: u64 = 6;

/// Backend-specific description for `PostgreSQL`.
const DESCRIPTION: &str = "Database MCP Server for PostgreSQL";

/// Backend-specific instructions for `PostgreSQL`.
const INSTRUCTIONS: &str = r"## Workflow

1. Call `list_databases` to discover available databases.
2. Call `list_tables` with a `database_name` to see its tables.
3. Call `get_table_schema` with `database_name` and `table_name` to inspect columns, types, and foreign keys before writing queries.
4. Use `read_query` for read-only SQL (SELECT, SHOW, EXPLAIN).
5. Use `write_query` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `create_database` to create a new database.
7. Use `drop_database` to drop an existing database.

Tools accept an optional `database_name` parameter to query across databases without reconnecting.

## Constraints

- The `write_query`, `create_database`, and `drop_database` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.";

/// `PostgreSQL` database handler.
///
/// The default connection pool is created with
/// [`PgPoolOptions::connect_lazy_with`], which defers all network I/O
/// until the first query. Non-default database pools are created on
/// demand via the moka [`Cache`].
#[derive(Clone)]
pub struct PostgresHandler {
    pub(crate) config: DatabaseConfig,
    pub(crate) default_db: String,
    default_pool: PgPool,
    pub(crate) pools: Cache<String, PgPool>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PostgresHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresHandler")
            .field("read_only", &self.config.read_only)
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl PostgresHandler {
    /// Creates a new `PostgreSQL` handler with a lazy connection pool.
    ///
    /// Does **not** establish a database connection. The default pool
    /// connects on-demand when the first query is executed. The MCP tool
    /// router is built once here and reused for every request.
    #[must_use]
    pub fn new(config: &DatabaseConfig) -> Self {
        // PostgreSQL defaults to a database named after the connecting user.
        let default_db = config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or_else(|| config.user.clone(), String::from);

        let default_pool = pool_options(config).connect_lazy_with(connect_options(config));

        info!(
            "PostgreSQL lazy connection pool created (max size: {})",
            config.max_pool_size
        );

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: PgPool, _cause| {
                tokio::spawn(async move {
                    pool.close().await;
                });
            })
            .build();

        Self {
            config: config.clone(),
            default_db,
            default_pool,
            pools,
            tool_router: build_tool_router(config.read_only),
        }
    }

    /// Wraps `name` in double quotes for safe use in `PostgreSQL` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }

    /// Returns a connection pool for the requested database.
    ///
    /// Resolves `None` or empty names to the default lazy pool. On a
    /// cache miss for a non-default database, a new lazy pool is created
    /// and cached. Evicted pools are closed via the cache's eviction
    /// listener.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidIdentifier`] if the database name fails
    /// validation.
    pub(crate) async fn get_pool(&self, database: Option<&str>) -> Result<PgPool, AppError> {
        let db_key = match database {
            Some(name) if !name.is_empty() => name,
            _ => return Ok(self.default_pool.clone()),
        };

        // Check if it's the default database by name.
        if db_key == self.default_db {
            return Ok(self.default_pool.clone());
        }

        // Non-default database: check cache first.
        if let Some(pool) = self.pools.get(db_key).await {
            return Ok(pool);
        }

        // Cache miss — validate then create a new lazy pool.
        validate_identifier(db_key)?;

        let config = self.config.clone();
        let db_key_owned = db_key.to_owned();

        let pool = self
            .pools
            .get_with(db_key_owned, async {
                let mut cfg = config;
                cfg.name = Some(db_key.to_owned());
                pool_options(&cfg).connect_lazy_with(connect_options(&cfg))
            })
            .await;

        Ok(pool)
    }
}

/// Builds [`PgPoolOptions`] with lifecycle defaults from a [`DatabaseConfig`].
fn pool_options(config: &DatabaseConfig) -> PgPoolOptions {
    let mut opts = PgPoolOptions::new()
        .max_connections(config.max_pool_size)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if let Some(timeout) = config.connection_timeout {
        opts = opts.acquire_timeout(Duration::from_secs(timeout));
    }

    opts
}

/// Builds [`PgConnectOptions`] from a [`DatabaseConfig`].
///
/// Uses [`PgConnectOptions::new_without_pgpass`] to avoid unintended
/// `PG*` environment variable influence, since our config already
/// resolves values from CLI/env.
fn connect_options(config: &DatabaseConfig) -> PgConnectOptions {
    let mut opts = PgConnectOptions::new_without_pgpass()
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

    if config.ssl {
        opts = if config.ssl_verify_cert {
            opts.ssl_mode(PgSslMode::VerifyCa)
        } else {
            opts.ssl_mode(PgSslMode::Require)
        };
        if let Some(ref ca) = config.ssl_ca {
            opts = opts.ssl_root_cert(ca);
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

        assert_eq!(opts.get_acquire_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn try_from_basic_config() {
        let config = base_config();
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
        assert_eq!(opts.get_port(), 5433);
        assert_eq!(opts.get_username(), "pgadmin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_ssl_require() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), PgSslMode::Require),
            "expected Require, got {:?}",
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
            matches!(opts.get_ssl_mode(), PgSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
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

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let config = base_config();
        let handler = PostgresHandler::new(&config);
        assert_eq!(handler.default_db, "mydb");
        // Pool exists but has no active connections (lazy).
        assert_eq!(handler.default_pool.size(), 0);
    }

    #[tokio::test]
    async fn new_defaults_db_to_username() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let handler = PostgresHandler::new(&config);
        assert_eq!(handler.default_db, "pgadmin");
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
