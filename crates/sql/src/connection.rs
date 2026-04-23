//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single trait every backend implements.
//! Backends provide pool resolution and timeout config; default method
//! implementations handle query execution.

use crate::SqlError;
use serde_json::Value;
use sqlx::{Decode, Execute, Executor, Row, Type};
use sqlx_json::{QueryResult as _, RowExt};

use crate::timeout::execute_with_timeout;

/// Unified query surface every backend tool handler uses.
///
/// Backends supply three required items — [`DB`](Connection::DB),
/// [`pool`](Connection::pool), and [`query_timeout`](Connection::query_timeout)
/// — and receive default implementations for query execution.
///
/// Query methods accept any [`sqlx::Execute`] value: a plain `&str` for
/// bindless statements (which routes through sqlx's unprepared text
/// protocol, required for statements like `MySQL` `USE`), or an
/// `sqlx::query(sql).bind(...)` value for parameterized statements.
///
/// # Errors
///
/// Query methods may return:
///
/// - [`SqlError::InvalidIdentifier`] — `database` failed identifier validation.
/// - [`SqlError::Connection`] — the underlying driver failed.
/// - [`SqlError::QueryTimeout`] — the query exceeded the configured timeout.
#[allow(async_fn_in_trait)]
pub trait Connection: Send + Sync
where
    for<'c> &'c mut <Self::DB as sqlx::Database>::Connection: Executor<'c, Database = Self::DB>,
    usize: sqlx::ColumnIndex<<Self::DB as sqlx::Database>::Row>,
    <Self::DB as sqlx::Database>::Row: RowExt,
    <Self::DB as sqlx::Database>::QueryResult: sqlx_json::QueryResult,
{
    /// The sqlx database driver type (e.g. `sqlx::MySql`).
    type DB: sqlx::Database;

    /// Resolves the connection pool for the given target database.
    ///
    /// # Errors
    ///
    /// - [`SqlError::InvalidIdentifier`] — `target` failed validation.
    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError>;

    /// Returns the configured query timeout in seconds, if any.
    fn query_timeout(&self) -> Option<u64>;

    /// Runs a statement that returns no meaningful rows.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn execute<'q, E>(&self, query: E, database: Option<&str>) -> Result<u64, SqlError>
    where
        E: 'q + Execute<'q, Self::DB> + Send,
    {
        let sql = query.sql().to_owned();
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), &sql, async move {
            Ok(pool.execute(query).await?.rows_affected())
        })
        .await
    }

    /// Runs a statement and collects every result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_json<'q, E>(&self, query: E, database: Option<&str>) -> Result<Vec<Value>, SqlError>
    where
        E: 'q + Execute<'q, Self::DB> + Send,
    {
        let sql = query.sql().to_owned();
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), &sql, async move {
            Ok(pool.fetch_all(query).await?.iter().map(RowExt::to_json).collect())
        })
        .await
    }

    /// Runs a query and extracts column 0 from the first row, if any.
    ///
    /// Returns `None` for both "no row returned" and "row where column 0
    /// is NULL" (decode errors are caught, not propagated).
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_optional<'q, E, T>(&self, query: E, database: Option<&str>) -> Result<Option<T>, SqlError>
    where
        E: 'q + Execute<'q, Self::DB> + Send,
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let sql = query.sql().to_owned();
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), &sql, async move {
            Ok(pool.fetch_optional(query).await?.and_then(|r| r.try_get(0usize).ok()))
        })
        .await
    }

    /// Runs a query and extracts the first column of every row.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_scalar<'q, E, T>(&self, query: E, database: Option<&str>) -> Result<Vec<T>, SqlError>
    where
        E: 'q + Execute<'q, Self::DB> + Send,
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let sql = query.sql().to_owned();
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), &sql, async move {
            let rows = pool.fetch_all(query).await?;
            rows.iter().map(|r| r.try_get(0usize)).collect()
        })
        .await
    }
}
