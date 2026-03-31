//! Database backend trait and enum dispatch.
//!
//! Defines the [`DatabaseBackend`] trait that all database backends
//! must implement for query execution and schema introspection.

use core::error::AppError;
use serde_json::Value;
use sqlparser::dialect::Dialect;

/// Operations every database backend must support.
pub trait DatabaseBackend: Send + Sync + Clone {
    /// Lists all accessible databases.
    fn list_databases(&self) -> impl Future<Output = Result<Vec<String>, AppError>> + Send;

    /// Lists all tables in a database.
    fn list_tables(&self, database: &str) -> impl Future<Output = Result<Vec<String>, AppError>> + Send;

    /// Returns column definitions with foreign key relationships for a table.
    fn get_table_schema(&self, database: &str, table: &str) -> impl Future<Output = Result<Value, AppError>> + Send;

    /// Executes a SQL query and returns rows as a JSON array.
    fn execute_query(&self, sql: &str, database: Option<&str>) -> impl Future<Output = Result<Value, AppError>> + Send;

    /// Creates a database if it doesn't exist.
    fn create_database(&self, name: &str) -> impl Future<Output = Result<Value, AppError>> + Send;

    /// Returns the SQL dialect for this backend.
    fn dialect(&self) -> Box<dyn Dialect>;

    /// Whether read-only mode is enabled.
    fn read_only(&self) -> bool;

    /// Whether this backend supports multiple databases.
    fn supports_multi_database(&self) -> bool;
}
