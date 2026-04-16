//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single trait every backend implements.
//! Backends provide pool resolution, identifier quoting config, and
//! timeout config; default method implementations handle query execution
//! and SQL quoting.

use database_mcp_server::AppError;
use serde_json::Value;
use sqlx::{Decode, Executor, Row, Type};
use sqlx_to_json::{QueryResult as _, RowExt};

use crate::identifier;
use crate::timeout::execute_with_timeout;

/// Unified query and quoting surface every backend tool handler uses.
///
/// Backends supply four required items — [`DB`](Connection::DB),
/// [`IDENTIFIER_QUOTE`](Connection::IDENTIFIER_QUOTE),
/// [`pool`](Connection::pool), and [`query_timeout`](Connection::query_timeout)
/// — and receive default implementations for query execution and SQL quoting.
///
/// # Errors
///
/// Query methods may return:
///
/// - [`AppError::InvalidIdentifier`] — `database` failed identifier validation.
/// - [`AppError::Connection`] — the underlying driver failed.
/// - [`AppError::QueryTimeout`] — the query exceeded the configured timeout.
#[allow(async_fn_in_trait)]
pub trait Connection: Send + Sync
where
    for<'c> &'c mut <Self::DB as sqlx::Database>::Connection: Executor<'c, Database = Self::DB>,
    usize: sqlx::ColumnIndex<<Self::DB as sqlx::Database>::Row>,
    <Self::DB as sqlx::Database>::Row: RowExt,
    <Self::DB as sqlx::Database>::QueryResult: sqlx_to_json::QueryResult,
{
    /// The sqlx database driver type (e.g. `sqlx::MySql`).
    type DB: sqlx::Database;

    /// Character used to quote identifiers (`` ` `` for `MySQL`, `"` for `PostgreSQL`/`SQLite`).
    const IDENTIFIER_QUOTE: char;

    /// Resolves the connection pool for the given target database.
    ///
    /// # Errors
    ///
    /// - [`AppError::InvalidIdentifier`] — `target` failed validation.
    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, AppError>;

    /// Returns the configured query timeout in seconds, if any.
    fn query_timeout(&self) -> Option<u64>;

    /// Runs a statement that returns no meaningful rows.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn execute(&self, query: &str, database: Option<&str>) -> Result<u64, AppError> {
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), query, async {
            Ok(pool.execute(query).await?.rows_affected())
        })
        .await
    }

    /// Runs a statement and collects every result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_json(&self, query: &str, database: Option<&str>) -> Result<Vec<Value>, AppError> {
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), query, async {
            Ok(pool.fetch_all(query).await?.iter().map(RowExt::to_json).collect())
        })
        .await
    }

    /// Runs a statement and returns at most one result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_optional<T>(&self, query: &str, database: Option<&str>) -> Result<Option<T>, AppError>
    where
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), query, async {
            pool.fetch_optional(query)
                .await?
                .as_ref()
                .map(|r| r.try_get(0usize))
                .transpose()
        })
        .await
    }

    /// Runs a query and extracts the first column of every row.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_scalar<T>(&self, query: &str, database: Option<&str>) -> Result<Vec<T>, AppError>
    where
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), query, async {
            let rows = pool.fetch_all(query).await?;
            rows.iter().map(|r| r.try_get(0usize)).collect()
        })
        .await
    }

    /// Wraps `name` in the backend's identifier quote character.
    fn quote_identifier(&self, name: &str) -> String {
        identifier::quote_identifier(name, Self::IDENTIFIER_QUOTE)
    }

    /// Wraps `value` in single quotes for use as a SQL string literal.
    fn quote_string(&self, value: &str) -> String {
        identifier::quote_string(value)
    }
}
