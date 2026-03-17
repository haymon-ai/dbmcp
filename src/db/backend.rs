//! Database backend trait, enum dispatch, and tool-level orchestration.
//!
//! Defines the [`DatabaseBackend`] trait and [`Backend`] enum that
//! dispatches to `MySQL`, `PostgreSQL`, or `SQLite` without dynamic dispatch.
//! The inherent `impl Backend` block provides MCP tool entry points that
//! combine input validation, delegation, and JSON formatting.

use crate::db::mysql::MysqlBackend;
use crate::db::postgres::PostgresBackend;
use crate::db::sqlite::SqliteBackend;
use crate::db::validation::validate_read_only_with_dialect;
use crate::error::AppError;
use enum_dispatch::enum_dispatch;
use serde_json::{Map, Value};
use sqlparser::dialect::Dialect;
use tracing::{error, info};

/// Operations every database backend must support.
#[allow(async_fn_in_trait)]
#[enum_dispatch]
pub trait DatabaseBackend {
    /// Lists all accessible databases.
    async fn list_databases(&self) -> Result<Vec<String>, AppError>;

    /// Lists all tables in a database.
    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError>;

    /// Returns column definitions for a table.
    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError>;

    /// Returns column definitions with foreign key relationships.
    async fn get_table_schema_with_relations(
        &self,
        database: &str,
        table: &str,
    ) -> Result<Value, AppError>;

    /// Executes a SQL query and returns rows as JSON objects.
    async fn execute_query(
        &self,
        sql: &str,
        database: Option<&str>,
    ) -> Result<Vec<Map<String, Value>>, AppError>;

    /// Creates a database if it doesn't exist.
    async fn create_database(&self, name: &str) -> Result<Value, AppError>;

    /// Returns the SQL dialect for this backend.
    fn dialect(&self) -> Box<dyn Dialect>;

    /// Whether read-only mode is enabled.
    fn read_only(&self) -> bool;
}

/// Concrete database backend — no dynamic dispatch.
#[derive(Debug, Clone)]
#[enum_dispatch(DatabaseBackend)]
pub enum Backend {
    /// `MySQL`/`MariaDB` via sqlx.
    Mysql(MysqlBackend),
    /// `PostgreSQL` via sqlx.
    Postgres(PostgresBackend),
    /// `SQLite` via sqlx.
    Sqlite(SqliteBackend),
}

// ---------------------------------------------------------------------------
// Tool-level orchestration (validation + delegation + JSON formatting)
// ---------------------------------------------------------------------------

impl Backend {
    /// Lists all accessible databases as a JSON array.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the backend query fails.
    pub async fn tool_list_databases(&self) -> Result<String, AppError> {
        info!("TOOL: list_databases called");
        let db_list = self.list_databases().await?;
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
    pub async fn tool_list_tables(&self, database_name: &str) -> Result<String, AppError> {
        info!("TOOL: list_tables called. database_name={database_name}");
        let table_list = match self.list_tables(database_name).await {
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
    pub async fn tool_get_table_schema(
        &self,
        database_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        info!(
            "TOOL: get_table_schema called. database_name={database_name}, table_name={table_name}"
        );
        let schema = self.get_table_schema(database_name, table_name).await?;
        info!("TOOL: get_table_schema completed");
        Ok(serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into()))
    }

    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if identifiers are invalid or the backend query fails.
    pub async fn tool_get_table_schema_with_relations(
        &self,
        database_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        info!(
            "TOOL: get_table_schema_with_relations called. database_name={database_name}, table_name={table_name}"
        );
        let result = self
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
        &self,
        sql_query: &str,
        database_name: &str,
        _parameters: Option<Vec<Value>>,
    ) -> Result<String, AppError> {
        info!(
            "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
            &sql_query[..sql_query.len().min(100)]
        );

        // Read-only validation with the backend's dialect
        if self.read_only() {
            let dialect = self.dialect();
            validate_read_only_with_dialect(sql_query, dialect.as_ref())?;
        }

        let db = if database_name.is_empty() {
            None
        } else {
            Some(database_name)
        };

        let results = self.execute_query(sql_query, db).await?;
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
    pub async fn tool_create_database(&self, database_name: &str) -> Result<String, AppError> {
        info!("TOOL: create_database called for database: '{database_name}'");
        let result = self.create_database(database_name).await?;
        info!("TOOL: create_database completed");
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()))
    }
}
