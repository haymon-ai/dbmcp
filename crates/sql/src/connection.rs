//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single entry point every backend tool
//! handler uses to run SQL.  Methods accept plain `&str` queries and an
//! optional target database name, returning unified JSON values.

use database_mcp_server::AppError;
use serde_json::Value;

/// Unified query surface every backend tool handler uses.
///
/// Three methods cover all SQL operations: [`execute`](Connection::execute),
/// [`fetch`](Connection::fetch), and [`fetch_optional`](Connection::fetch_optional).
///
/// # Errors
///
/// All methods may return:
///
/// - [`AppError::InvalidIdentifier`] — `database` failed identifier validation.
/// - [`AppError::Connection`] — the underlying driver failed.
/// - [`AppError::QueryTimeout`] — the query exceeded the configured timeout.
pub trait Connection: Send + Sync {
    /// Runs a statement that returns no meaningful rows.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    fn execute(&self, query: &str, database: Option<&str>) -> impl Future<Output = Result<u64, AppError>> + Send;

    /// Runs a statement and collects every result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    fn fetch(&self, query: &str, database: Option<&str>) -> impl Future<Output = Result<Vec<Value>, AppError>> + Send;

    /// Runs a statement and returns at most one result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    fn fetch_optional(
        &self,
        query: &str,
        database: Option<&str>,
    ) -> impl Future<Output = Result<Option<Value>, AppError>> + Send;
}
