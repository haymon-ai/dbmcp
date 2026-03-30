//! Database backend trait and enum dispatch.
//!
//! Defines the [`DatabaseBackend`] trait and [`Backend`] enum that
//! dispatches to `MySQL`, `PostgreSQL`, or `SQLite` without dynamic dispatch.

use crate::db::mysql::MysqlBackend;
use crate::db::postgres::PostgresBackend;
use crate::db::sqlite::SqliteBackend;
use crate::error::AppError;
use serde_json::Value;
use sqlparser::dialect::Dialect;

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
