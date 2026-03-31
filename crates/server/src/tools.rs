//! Tool registration and route definitions.
//!
//! Builds the [`ToolRouter`] based on backend capabilities and
//! provides individual [`ToolRoute`] constructors for each MCP tool.

use std::sync::Arc;

use rmcp::handler::server::common::{FromContextPart, schema_for_empty_input, schema_for_type};
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Tool, ToolAnnotations};
use rmcp::schemars::JsonSchema;
use serde_json::Map as JsonObject;

use crate::server::Server;
use crate::types::{CreateDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};

/// Returns the JSON Schema for `Parameters<T>`.
fn schema_for<T: JsonSchema + 'static>() -> Arc<JsonObject<String, serde_json::Value>> {
    schema_for_type::<Parameters<T>>()
}

/// Builds the [`ToolRouter`] for the given backend.
///
/// All backends share the same read tools. Write tools are added
/// when not in read-only mode. `list_databases` and `create_database`
/// are excluded for backends that do not support multiple databases.
pub fn build_tool_router<B: backend::DatabaseBackend + 'static>(backend: &B) -> ToolRouter<Server<B>> {
    let mut router = ToolRouter::new();

    if backend.supports_multi_database() {
        router.add_route(list_databases_route());
    }

    router.add_route(list_tables_route());
    router.add_route(get_table_schema_route());
    router.add_route(read_query_route());

    if backend.read_only() {
        return router;
    }

    router.add_route(write_query_route());

    if backend.supports_multi_database() {
        router.add_route(create_database_route());
    }

    router
}

/// Route for the `list_databases` tool.
#[must_use]
fn list_databases_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |ctx: ToolCallContext<'_, Server<B>>| {
            let server = ctx.service;
            Box::pin(async move { server.list_databases().await })
        },
    )
}

/// Route for the `list_tables` tool.
#[must_use]
fn list_tables_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |mut ctx: ToolCallContext<'_, Server<B>>| {
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
fn get_table_schema_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |mut ctx: ToolCallContext<'_, Server<B>>| {
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
fn read_query_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |mut ctx: ToolCallContext<'_, Server<B>>| {
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
fn write_query_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |mut ctx: ToolCallContext<'_, Server<B>>| {
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
fn create_database_route<B: backend::DatabaseBackend + 'static>() -> ToolRoute<Server<B>> {
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
        |mut ctx: ToolCallContext<'_, Server<B>>| {
            let params = Parameters::<CreateDatabaseRequest>::from_context_part(&mut ctx);
            let server = ctx.service;
            Box::pin(async move {
                let params = params?;
                server.create_database(params).await
            })
        },
    )
}
