//! Concrete backend enum — composition root for database dispatch.
//!
//! Wraps per-backend crate types into a single [`Backend`] enum and
//! manually dispatches [`backend::DatabaseBackend`] trait methods.

use backend::DatabaseBackend;
use mcp_core::error::AppError;
use serde_json::Value;
use sqlparser::dialect::Dialect;

/// Concrete database backend — dispatches to the active variant.
///
/// Only one instance exists for the program lifetime, so the size
/// difference between variants is irrelevant.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Backend {
    /// `MySQL`/`MariaDB` via sqlx.
    Mysql(mysql::MysqlBackend),
    /// `PostgreSQL` via sqlx.
    Postgres(postgres::PostgresBackend),
    /// `SQLite` via sqlx.
    Sqlite(sqlite::SqliteBackend),
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

    fn supports_multi_database(&self) -> bool {
        match self {
            Self::Mysql(b) => b.supports_multi_database(),
            Self::Postgres(b) => b.supports_multi_database(),
            Self::Sqlite(b) => b.supports_multi_database(),
        }
    }
}
