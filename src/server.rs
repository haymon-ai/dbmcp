//! MCP server setup, tool definitions, handlers, and transport dispatch.
//!
//! Defines [`Server`] which implements the MCP `ServerHandler` trait.
//! Tool registration uses [`build_tool_router`] to conditionally include
//! tools based on the database backend and read-only setting.

use std::sync::Arc;

use crate::db::backend::{Backend, DatabaseBackend};
use crate::db::validation::validate_read_only_with_dialect;
use rmcp::handler::server::common::{FromContextPart, schema_for_empty_input, schema_for_type};
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde::Deserialize;
use serde_json::Map as JsonObject;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request to list tables in a database.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    #[schemars(
        description = "The database name to list tables from. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
}

/// Request to get a table's schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTableSchemaRequest {
    #[schemars(
        description = "The database name containing the table. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
    #[schemars(
        description = "The table name to inspect. Use list_tables first to see available tables in the database."
    )]
    pub table_name: String,
}

/// Request for `read_query` and `write_query` tools.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    #[schemars(description = "The SQL query to execute.")]
    pub sql_query: String,
    #[schemars(
        description = "The database to run the query against. Required. Use list_databases first to see available databases."
    )]
    pub database_name: String,
}

/// Request to create a database.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    #[schemars(
        description = "Name of the database to create. Must contain only alphanumeric characters and underscores."
    )]
    pub database_name: String,
}

// ---------------------------------------------------------------------------
// Tool route definitions
// ---------------------------------------------------------------------------

/// Returns the JSON Schema for `Parameters<T>`.
fn schema_for<T: JsonSchema + 'static>() -> Arc<JsonObject<String, serde_json::Value>> {
    schema_for_type::<Parameters<T>>()
}

/// Route for the `list_databases` tool.
#[must_use]
fn list_databases_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "list_databases",
            "List all accessible databases on the connected database server. Call this first to discover available database names.",
            schema_for_empty_input(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        ),
        |ctx: ToolCallContext<'_, Server>| {
            let server = ctx.service;
            Box::pin(async move { server.list_databases().await })
        },
    )
}

/// Route for the `list_tables` tool.
#[must_use]
fn list_tables_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "list_tables",
            "List all tables in a specific database. Requires database_name from list_databases.",
            schema_for::<ListTablesRequest>(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        ),
        |mut ctx: ToolCallContext<'_, Server>| {
            let params = Parameters::<ListTablesRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.list_tables(params).await
            })
        },
    )
}

/// Route for the `get_table_schema` tool.
#[must_use]
fn get_table_schema_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "get_table_schema",
            "Get column definitions (type, nullable, key, default) and foreign key relationships for a table. Requires database_name and table_name.",
            schema_for::<GetTableSchemaRequest>(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        ),
        |mut ctx: ToolCallContext<'_, Server>| {
            let params = Parameters::<GetTableSchemaRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.get_table_schema(params).await
            })
        },
    )
}

/// Route for the `read_query` tool.
#[must_use]
fn read_query_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "read_query",
            "Execute a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).",
            schema_for::<QueryRequest>(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        ),
        |mut ctx: ToolCallContext<'_, Server>| {
            let params = Parameters::<QueryRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.read_query(params).await
            })
        },
    )
}

/// Route for the `write_query` tool.
#[must_use]
fn write_query_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "write_query",
            "Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).",
            schema_for::<QueryRequest>(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        ),
        |mut ctx: ToolCallContext<'_, Server>| {
            let params = Parameters::<QueryRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.write_query(params).await
            })
        },
    )
}

/// Route for the `create_database` tool.
#[must_use]
fn create_database_route() -> ToolRoute<Server> {
    ToolRoute::new_dyn(
        Tool::new(
            "create_database",
            "Create a new database. Not supported for SQLite.",
            schema_for::<CreateDatabaseRequest>(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(false)
                .idempotent(false)
                .open_world(false),
        ),
        |mut ctx: ToolCallContext<'_, Server>| {
            let params = Parameters::<CreateDatabaseRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.create_database(params).await
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

fn map_error(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// MCP server backed by a database backend.
#[derive(Clone)]
pub struct Server {
    /// The active database backend.
    pub backend: Backend,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl Server {
    /// Creates a new MCP server with the given database backend.
    ///
    /// The tool router is built based on the backend's capabilities
    /// and read-only setting. `SQLite` does not support `create_database`.
    #[must_use]
    pub fn new(backend: Backend) -> Self {
        let tool_router = Self::build_tool_router(&backend);
        Self { backend, tool_router }
    }

    /// Builds the [`ToolRouter`] for the given backend.
    ///
    /// All backends share the same 5 read tools. Write tools are added
    /// when not in read-only mode. `create_database` is excluded for
    /// `SQLite` since it has no server-side database management.
    fn build_tool_router(backend: &Backend) -> ToolRouter<Self> {
        let mut router = ToolRouter::new();

        if !matches!(backend, Backend::Sqlite(_)) {
            router.add_route(list_databases_route());
        }

        router.add_route(list_tables_route());
        router.add_route(get_table_schema_route());
        router.add_route(read_query_route());

        if backend.read_only() {
            return router;
        }

        router.add_route(write_query_route());

        if !matches!(backend, Backend::Sqlite(_)) {
            router.add_route(create_database_route());
        }

        router
    }
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

impl Server {
    /// List all accessible databases on the connected database server.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_databases(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.backend.tool_list_databases().await.map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// List all tables in a specific database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn list_tables(&self, req: Parameters<ListTablesRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_list_tables(&req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Get column definitions and foreign key relationships for a table.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn get_table_schema(&self, req: Parameters<GetTableSchemaRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_get_table_schema(&req.0.database_name, &req.0.table_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Execute a read-only SQL query with AST validation.
    ///
    /// Always enforces SQL validation (only SELECT, SHOW, DESCRIBE,
    /// USE, EXPLAIN allowed) as defence-in-depth, regardless of the
    /// server's read-only setting.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if SQL validation fails or the query errors.
    pub async fn read_query(&self, req: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        // Scope the dialect so the non-Send Box<dyn Dialect> is dropped before .await
        {
            let dialect = self.backend.dialect();
            validate_read_only_with_dialect(&req.0.sql_query, dialect.as_ref()).map_err(map_error)?;
        }

        let result = self
            .backend
            .tool_execute_sql(&req.0.sql_query, &req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Execute a write SQL query.
    ///
    /// No SQL type validation — the tool boundary is the access control.
    /// This tool is only registered when the server is not in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the query fails.
    pub async fn write_query(&self, req: Parameters<QueryRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_execute_sql(&req.0.sql_query, &req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Create a new database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] if the backend query fails.
    pub async fn create_database(&self, req: Parameters<CreateDatabaseRequest>) -> Result<CallToolResult, ErrorData> {
        let result = self
            .backend
            .tool_create_database(&req.0.database_name)
            .await
            .map_err(map_error)?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler
// ---------------------------------------------------------------------------

impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Database MCP Server - provides database exploration and query tools for MySQL, MariaDB, PostgreSQL, and SQLite",
            )
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

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn map_error_converts_display_to_error_data() {
        let err = AppError::ReadOnlyViolation;
        let mapped = map_error(err);
        assert!(
            mapped.message.contains("read-only"),
            "mapped error should preserve the original message"
        );
    }

    #[test]
    fn map_error_converts_string_to_error_data() {
        let mapped = map_error("something went wrong");
        assert_eq!(mapped.message, "something went wrong");
    }

    #[test]
    fn get_info_returns_tools_capability_and_server_info() {
        let info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
        assert!(info.capabilities.tools.is_some(), "tools capability should be enabled");
        assert_eq!(info.server_info.name, "database-mcp");
        assert!(!info.server_info.version.is_empty(), "version should not be empty");
    }

    // --- route definition tests ---

    #[test]
    fn list_databases_route_has_correct_name_and_empty_schema() {
        let route = list_databases_route();
        assert_eq!(route.attr.name.as_ref(), "list_databases");
        assert!(
            route
                .attr
                .description
                .as_deref()
                .is_some_and(|d| d.contains("List all accessible databases")),
            "description should mention listing databases"
        );
        let schema = &route.attr.input_schema;
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
    }

    #[test]
    fn list_tables_route_has_correct_name_and_schema() {
        let route = list_tables_route();
        assert_eq!(route.attr.name.as_ref(), "list_tables");
        let props = route.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(
            props.is_some_and(|p| p.contains_key("database_name")),
            "schema should have database_name property"
        );
    }

    #[test]
    fn get_table_schema_route_has_correct_name_and_schema() {
        let route = get_table_schema_route();
        assert_eq!(route.attr.name.as_ref(), "get_table_schema");
        let props = route.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(
            props.is_some_and(|p| p.contains_key("database_name") && p.contains_key("table_name")),
            "schema should have database_name and table_name properties"
        );
    }

    #[test]
    fn read_query_route_has_correct_name_and_schema() {
        let route = read_query_route();
        assert_eq!(route.attr.name.as_ref(), "read_query");
        assert!(
            route
                .attr
                .description
                .as_deref()
                .is_some_and(|d| d.contains("read-only")),
            "description should mention read-only"
        );
        let props = route.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(
            props.is_some_and(|p| p.contains_key("sql_query") && p.contains_key("database_name")),
            "schema should have sql_query and database_name properties"
        );
    }

    #[test]
    fn write_query_route_has_correct_name_and_schema() {
        let route = write_query_route();
        assert_eq!(route.attr.name.as_ref(), "write_query");
        assert!(
            route.attr.description.as_deref().is_some_and(|d| d.contains("write")),
            "description should mention write"
        );
        let props = route.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(
            props.is_some_and(|p| p.contains_key("sql_query") && p.contains_key("database_name")),
            "schema should have sql_query and database_name properties"
        );
    }

    #[test]
    fn create_database_route_has_correct_name_and_schema() {
        let route = create_database_route();
        assert_eq!(route.attr.name.as_ref(), "create_database");
        assert!(
            route.attr.description.as_deref().is_some_and(|d| d.contains("SQLite")),
            "description should mention SQLite not supported"
        );
        let props = route.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(
            props.is_some_and(|p| p.contains_key("database_name")),
            "schema should have database_name property"
        );
    }

    #[test]
    fn read_and_write_query_share_same_schema_shape() {
        let read = read_query_route();
        let write = write_query_route();
        let read_props = read.attr.input_schema.get("properties").and_then(|v| v.as_object());
        let write_props = write.attr.input_schema.get("properties").and_then(|v| v.as_object());
        assert!(read_props.is_some());
        assert_eq!(
            read_props.map(|p| p.keys().collect::<std::collections::BTreeSet<_>>()),
            write_props.map(|p| p.keys().collect::<std::collections::BTreeSet<_>>()),
            "read_query and write_query should have the same input schema properties"
        );
    }

    // --- build_tool_router tests ---
    //
    // Uses SQLite in-memory backends since they're cheap to construct.
    // MySQL/Postgres router behavior is verified by integration tests.

    use crate::db::sqlite::SqliteBackend;

    fn router_tool_names(backend: &Backend) -> Vec<String> {
        Server::build_tool_router(backend)
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    fn sqlite_backend(read_only: bool) -> Backend {
        Backend::Sqlite(SqliteBackend::in_memory(read_only))
    }

    // --- tool annotation tests ---

    /// Unwraps the annotations from a tool route, panicking if absent.
    fn annotations(route: &ToolRoute<Server>) -> &ToolAnnotations {
        route.attr.annotations.as_ref().expect("tool should have annotations")
    }

    #[test]
    fn list_databases_annotations_are_read_only_closed_world() {
        let route = list_databases_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
        assert_eq!(ann.idempotent_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
    }

    #[test]
    fn list_tables_annotations_are_read_only_closed_world() {
        let route = list_tables_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
        assert_eq!(ann.idempotent_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
    }

    #[test]
    fn get_table_schema_annotations_are_read_only_closed_world() {
        let route = get_table_schema_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
        assert_eq!(ann.idempotent_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
    }

    #[test]
    fn read_query_annotations_are_read_only_open_world() {
        let route = read_query_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
        assert_eq!(ann.idempotent_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(true));
    }

    #[test]
    fn write_query_annotations_are_destructive_open_world() {
        let route = write_query_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(true));
        assert_eq!(ann.idempotent_hint, Some(false));
        assert_eq!(ann.open_world_hint, Some(true));
    }

    #[test]
    fn create_database_annotations_are_non_destructive_closed_world() {
        let route = create_database_route();
        let ann = annotations(&route);
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(false));
        assert_eq!(ann.idempotent_hint, Some(false));
        assert_eq!(ann.open_world_hint, Some(false));
    }

    #[tokio::test]
    async fn all_router_tools_have_annotations() {
        let backend = sqlite_backend(false);
        let tools = Server::build_tool_router(&backend).list_all();
        for tool in &tools {
            assert!(
                tool.annotations.is_some(),
                "tool '{}' should have annotations",
                tool.name
            );
        }
    }

    // --- build_tool_router tests ---
    //
    // Uses SQLite in-memory backends since they're cheap to construct.
    // MySQL/Postgres router behavior is verified by integration tests.

    #[tokio::test]
    async fn router_sqlite_read_only_returns_3_tools() {
        let names = router_tool_names(&sqlite_backend(true));
        assert_eq!(names.len(), 3);
        assert!(!names.contains(&"list_databases".to_string()));
        assert!(names.contains(&"list_tables".to_string()));
        assert!(names.contains(&"get_table_schema".to_string()));
        assert!(names.contains(&"read_query".to_string()));
    }

    #[tokio::test]
    async fn router_sqlite_read_write_returns_4_tools() {
        let names = router_tool_names(&sqlite_backend(false));
        assert_eq!(names.len(), 4);
        assert!(!names.contains(&"list_databases".to_string()));
        assert!(names.contains(&"write_query".to_string()));
        assert!(!names.contains(&"create_database".to_string()));
    }
}
