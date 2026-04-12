//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single trait every backend implements.
//! Backends provide pool resolution, identifier quoting config, and
//! timeout config; default method implementations handle query execution
//! and SQL quoting.

use database_mcp_server::AppError;
use serde_json::Value;
use sqlx::Executor;
use sqlx_to_json::QueryResult as _;

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
pub trait Connection: Send + Sync {
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
    async fn execute(&self, query: &str, database: Option<&str>) -> Result<u64, AppError>
    where
        for<'c> &'c mut <Self::DB as sqlx::Database>::Connection: Executor<'c, Database = Self::DB>,
        <Self::DB as sqlx::Database>::QueryResult: sqlx_to_json::QueryResult,
    {
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        execute_with_timeout(self.query_timeout(), query, async move {
            let mut conn = pool.acquire().await?;
            let result = (&mut *conn).execute(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(result.rows_affected())
        })
        .await
    }

    /// Runs a statement and collects every result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch(&self, query: &str, database: Option<&str>) -> Result<Vec<Value>, AppError>
    where
        for<'c> &'c mut <Self::DB as sqlx::Database>::Connection: Executor<'c, Database = Self::DB>,
        <Self::DB as sqlx::Database>::Row: sqlx_to_json::RowExt,
    {
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        execute_with_timeout(self.query_timeout(), query, async move {
            let mut conn = pool.acquire().await?;
            let rows = (&mut *conn).fetch_all(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(rows.iter().map(sqlx_to_json::RowExt::to_json).collect())
        })
        .await
    }

    /// Runs a statement and returns at most one result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_optional(&self, query: &str, database: Option<&str>) -> Result<Option<Value>, AppError>
    where
        for<'c> &'c mut <Self::DB as sqlx::Database>::Connection: Executor<'c, Database = Self::DB>,
        <Self::DB as sqlx::Database>::Row: sqlx_to_json::RowExt,
    {
        let pool = self.pool(database).await?;
        let sql = query.to_owned();
        execute_with_timeout(self.query_timeout(), query, async move {
            let mut conn = pool.acquire().await?;
            let row = (&mut *conn).fetch_optional(sql.as_str()).await?;
            Ok::<_, sqlx::Error>(row.as_ref().map(sqlx_to_json::RowExt::to_json))
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
