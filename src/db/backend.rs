//! Database backend trait and enum dispatch.
//!
//! Defines the [`DatabaseBackend`] trait and [`Backend`] enum that
//! dispatches to `MySQL`, `PostgreSQL`, or `SQLite` without dynamic dispatch.

use crate::db::mysql::MysqlBackend;
use crate::db::postgres::PostgresBackend;
use crate::db::sqlite::SqliteBackend;
use crate::error::AppError;
use enum_dispatch::enum_dispatch;
use serde_json::{Map, Value};
use sqlparser::dialect::Dialect;

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
