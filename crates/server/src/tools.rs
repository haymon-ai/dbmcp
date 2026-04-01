//! Shared tool implementation functions.
//!
//! Extracts the common logging, validation, and serialization logic
//! from per-backend MCP tool handlers into reusable functions.

use database_mcp_backend::error::AppError;
use rmcp::model::ErrorData;
use serde::Serialize;
use serde_json::Value;
use tracing::info;

use crate::map_error;

/// Executes a `list_databases` tool call.
///
/// # Errors
///
/// Returns [`ErrorData`] if the backend query or JSON serialization fails.
pub async fn list_databases(list_fn: impl Future<Output = Result<Vec<String>, AppError>>) -> Result<String, ErrorData> {
    info!("TOOL: list_databases called");
    let db_list = list_fn.await.map_err(map_error)?;
    info!("TOOL: list_databases completed. Databases found: {}", db_list.len());
    serde_json::to_string_pretty(&db_list).map_err(map_error)
}

/// Executes a `list_tables` tool call.
///
/// # Errors
///
/// Returns [`ErrorData`] if the backend query or JSON serialization fails.
pub async fn list_tables(
    list_fn: impl Future<Output = Result<Vec<String>, AppError>>,
    database_name: &str,
) -> Result<String, ErrorData> {
    info!("TOOL: list_tables called. database_name={database_name}");
    let table_list = list_fn.await.map_err(map_error)?;
    info!("TOOL: list_tables completed. Tables found: {}", table_list.len());
    serde_json::to_string_pretty(&table_list).map_err(map_error)
}

/// Executes a `get_table_schema` tool call.
///
/// # Errors
///
/// Returns [`ErrorData`] if the backend query or JSON serialization fails.
pub async fn get_table_schema(
    schema_fn: impl Future<Output = Result<impl Serialize, AppError>>,
    database_name: &str,
    table_name: &str,
) -> Result<String, ErrorData> {
    info!("TOOL: get_table_schema called. database_name={database_name}, table_name={table_name}");
    let schema = schema_fn.await.map_err(map_error)?;
    info!("TOOL: get_table_schema completed");
    serde_json::to_string_pretty(&schema).map_err(map_error)
}

/// Executes a `read_query` tool call with read-only validation.
///
/// The `validate` closure performs backend-specific SQL validation
/// (e.g. read-only enforcement with the appropriate SQL dialect).
///
/// # Errors
///
/// Returns [`ErrorData`] if validation, the backend query, or JSON serialization fails.
pub async fn read_query(
    query_fn: impl Future<Output = Result<Value, AppError>>,
    sql_query: &str,
    database_name: &str,
    validate: impl FnOnce(&str) -> Result<(), AppError>,
) -> Result<String, ErrorData> {
    info!(
        "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
        &sql_query[..sql_query.len().min(100)]
    );

    validate(sql_query).map_err(map_error)?;

    let results = query_fn.await.map_err(map_error)?;
    let row_count = results.as_array().map_or(0, Vec::len);
    info!("TOOL: execute_sql completed. Rows returned: {row_count}");
    serde_json::to_string_pretty(&results).map_err(map_error)
}

/// Executes a `write_query` tool call.
///
/// # Errors
///
/// Returns [`ErrorData`] if the backend query or JSON serialization fails.
pub async fn write_query(
    query_fn: impl Future<Output = Result<Value, AppError>>,
    sql_query: &str,
    database_name: &str,
) -> Result<String, ErrorData> {
    info!(
        "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
        &sql_query[..sql_query.len().min(100)]
    );

    let results = query_fn.await.map_err(map_error)?;
    let row_count = results.as_array().map_or(0, Vec::len);
    info!("TOOL: execute_sql completed. Rows returned: {row_count}");
    serde_json::to_string_pretty(&results).map_err(map_error)
}

/// Executes a `create_database` tool call.
///
/// # Errors
///
/// Returns [`ErrorData`] if the backend query or JSON serialization fails.
pub async fn create_database(
    create_fn: impl Future<Output = Result<Value, AppError>>,
    database_name: &str,
) -> Result<String, ErrorData> {
    info!("TOOL: create_database called for database: '{database_name}'");
    let result = create_fn.await.map_err(map_error)?;
    info!("TOOL: create_database completed");
    serde_json::to_string_pretty(&result).map_err(map_error)
}

/// Resolves an empty database name to `None`.
#[must_use]
pub fn resolve_database(name: &str) -> Option<&str> {
    if name.is_empty() { None } else { Some(name) }
}
