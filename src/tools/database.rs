//! Database tool implementations for the MCP server.
//!
//! Provides 6 tools: `list_databases`, `list_tables`, `get_table_schema`,
//! `get_table_schema_with_relations`, `execute_sql`, and `create_database`.
//! All tools delegate to the active [`Backend`].

use crate::db::backend::{Backend, DatabaseBackend};
use crate::db::identifier::validate_identifier;
use crate::db::validation::validate_read_only_with_dialect;
use crate::error::AppError;
use serde_json::Value;
use tracing::{error, info};

/// Lists all accessible databases as a JSON array.
///
/// # Errors
///
/// Returns [`AppError`] if the backend query fails.
pub async fn list_databases(backend: &Backend) -> Result<String, AppError> {
    info!("TOOL: list_databases called");
    let db_list = backend.list_databases().await?;
    info!(
        "TOOL: list_databases completed. Databases found: {}",
        db_list.len()
    );
    Ok(serde_json::to_string_pretty(&db_list).unwrap_or_else(|_| "[]".into()))
}

/// Lists all tables in a database as a JSON array.
///
/// # Errors
///
/// Returns [`AppError`] if the identifier is invalid or the backend query fails.
pub async fn list_tables(backend: &Backend, database_name: &str) -> Result<String, AppError> {
    info!("TOOL: list_tables called. database_name={database_name}");
    validate_identifier(database_name)?;
    let table_list = match backend.list_tables(database_name).await {
        Ok(t) => t,
        Err(e) => {
            error!("TOOL ERROR: list_tables failed for database_name={database_name}: {e}");
            return Err(e);
        }
    };
    info!(
        "TOOL: list_tables completed. Tables found: {}",
        table_list.len()
    );
    Ok(serde_json::to_string_pretty(&table_list).unwrap_or_else(|_| "[]".into()))
}

/// Returns column definitions for a table as JSON.
///
/// # Errors
///
/// Returns [`AppError`] if identifiers are invalid or the backend query fails.
pub async fn get_table_schema(
    backend: &Backend,
    database_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    info!("TOOL: get_table_schema called. database_name={database_name}, table_name={table_name}");
    validate_identifier(database_name)?;
    validate_identifier(table_name)?;
    let schema = backend.get_table_schema(database_name, table_name).await?;
    info!("TOOL: get_table_schema completed");
    Ok(serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into()))
}

/// Returns column definitions with foreign key relationships.
///
/// # Errors
///
/// Returns [`AppError`] if identifiers are invalid or the backend query fails.
pub async fn get_table_schema_with_relations(
    backend: &Backend,
    database_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    info!(
        "TOOL: get_table_schema_with_relations called. database_name={database_name}, table_name={table_name}"
    );
    validate_identifier(database_name)?;
    validate_identifier(table_name)?;
    let result = backend
        .get_table_schema_with_relations(database_name, table_name)
        .await?;
    info!("TOOL: get_table_schema_with_relations completed");
    Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()))
}

/// Executes a user-provided SQL query with read-only validation.
///
/// # Errors
///
/// Returns [`AppError`] if the identifier is invalid, the query is blocked
/// by read-only mode, or the backend query fails.
pub async fn tool_execute_sql(
    backend: &Backend,
    sql_query: &str,
    database_name: &str,
    _parameters: Option<Vec<Value>>,
) -> Result<String, AppError> {
    info!(
        "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
        &sql_query[..sql_query.len().min(100)]
    );

    if !database_name.is_empty() {
        validate_identifier(database_name)?;
    }

    // Read-only validation with the backend's dialect
    if backend.read_only() {
        let dialect = backend.dialect();
        validate_read_only_with_dialect(sql_query, dialect.as_ref())?;
    }

    let db = if database_name.is_empty() {
        None
    } else {
        Some(database_name)
    };

    let results = backend.execute_query(sql_query, db).await?;
    info!(
        "TOOL: execute_sql completed. Rows returned: {}",
        results.len()
    );
    Ok(serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".into()))
}

/// Creates a database if it does not already exist.
///
/// # Errors
///
/// Returns [`AppError`] if the identifier is invalid, the server is in
/// read-only mode, or the backend query fails.
pub async fn create_database(backend: &Backend, database_name: &str) -> Result<String, AppError> {
    info!("TOOL: create_database called for database: '{database_name}'");
    validate_identifier(database_name)?;
    let result = backend.create_database(database_name).await?;
    info!("TOOL: create_database completed");
    Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()))
}
