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
use serde_json::Value;
use sqlparser::dialect::Dialect;
use tracing::{error, info};

/// Operations every database backend must support.
#[allow(async_fn_in_trait)]
pub trait DatabaseBackend {
    /// Lists all accessible databases.
    async fn list_databases(&self) -> Result<Vec<String>, AppError>;

    /// Lists all tables in a database.
    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError>;

    /// Returns column definitions with foreign key relationships for a table.
    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError>;

    /// Executes a SQL query and returns rows as a JSON array.
    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError>;

    /// Creates a database if it doesn't exist.
    async fn create_database(&self, name: &str) -> Result<Value, AppError>;

    /// Returns the SQL dialect for this backend.
    fn dialect(&self) -> Box<dyn Dialect>;

    /// Whether read-only mode is enabled.
    fn read_only(&self) -> bool;
}

/// Concrete database backend — dispatches to the active variant.
///
/// Only one instance exists for the program lifetime, so the size
/// difference between variants is irrelevant.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Backend {
    /// `MySQL`/`MariaDB` via sqlx.
    Mysql(MysqlBackend),
    /// `PostgreSQL` via sqlx.
    Postgres(PostgresBackend),
    /// `SQLite` via sqlx.
    Sqlite(SqliteBackend),
}

impl DatabaseBackend for Backend {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        match self {
            Self::Mysql(b) => b.list_databases().await,
            Self::Postgres(b) => b.list_databases().await,
            Self::Sqlite(b) => b.list_databases().await,
        }
    }

    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        match self {
            Self::Mysql(b) => b.list_tables(database).await,
            Self::Postgres(b) => b.list_tables(database).await,
            Self::Sqlite(b) => b.list_tables(database).await,
        }
    }

    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError> {
        match self {
            Self::Mysql(b) => b.get_table_schema(database, table).await,
            Self::Postgres(b) => b.get_table_schema(database, table).await,
            Self::Sqlite(b) => b.get_table_schema(database, table).await,
        }
    }

    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        match self {
            Self::Mysql(b) => b.execute_query(sql, database).await,
            Self::Postgres(b) => b.execute_query(sql, database).await,
            Self::Sqlite(b) => b.execute_query(sql, database).await,
        }
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        match self {
            Self::Mysql(b) => b.create_database(name).await,
            Self::Postgres(b) => b.create_database(name).await,
            Self::Sqlite(b) => b.create_database(name).await,
        }
    }

    fn dialect(&self) -> Box<dyn Dialect> {
        match self {
            Self::Mysql(b) => b.dialect(),
            Self::Postgres(b) => b.dialect(),
            Self::Sqlite(b) => b.dialect(),
        }
    }

    fn read_only(&self) -> bool {
        match self {
            Self::Mysql(b) => b.read_only(),
            Self::Postgres(b) => b.read_only(),
            Self::Sqlite(b) => b.read_only(),
        }
    }
}

impl Backend {
    /// Lists all accessible databases as a JSON array.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the backend query fails.
    pub async fn tool_list_databases(&self) -> Result<String, AppError> {
        info!("TOOL: list_databases called");
        let db_list = self.list_databases().await?;
        info!("TOOL: list_databases completed. Databases found: {}", db_list.len());
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
        info!("TOOL: list_tables completed. Tables found: {}", table_list.len());
        Ok(serde_json::to_string_pretty(&table_list).unwrap_or_else(|_| "[]".into()))
    }

    /// Returns column definitions with foreign key relationships as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if identifiers are invalid or the backend query fails.
    pub async fn tool_get_table_schema(&self, database_name: &str, table_name: &str) -> Result<String, AppError> {
        info!("TOOL: get_table_schema called. database_name={database_name}, table_name={table_name}");
        let schema = self.get_table_schema(database_name, table_name).await?;
        info!("TOOL: get_table_schema completed");
        Ok(serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into()))
    }

    /// Executes a SQL query and returns results as a JSON string.
    ///
    /// Includes read-only validation as defence-in-depth. The `read_query`
    /// tool handler also validates, but this ensures safety even when
    /// calling `tool_execute_sql` directly (e.g. from integration tests).
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query is blocked by read-only mode
    /// or the backend query fails.
    pub async fn tool_execute_sql(&self, sql_query: &str, database_name: &str) -> Result<String, AppError> {
        info!(
            "TOOL: execute_sql called. database_name={database_name}, sql_query={}",
            &sql_query[..sql_query.len().min(100)]
        );

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
        let row_count = results.as_array().map_or(0, Vec::len);
        info!("TOOL: execute_sql completed. Rows returned: {row_count}");
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
