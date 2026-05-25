//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single trait every backend implements.
//! Backends provide pool resolution and timeout config; default method
//! implementations handle query execution.

use crate::SqlError;
use serde_json::Value;
use sqlx::{AssertSqlSafe, Decode, Execute, Executor, FromRow, Row, SqlSafeStr, SqlStr, Type};
use sqlx_json::{QueryResult as _, RowExt};

use crate::timeout::execute_with_timeout;

/// Splits a query into its SQL text and bound arguments for safe execution.
///
/// Lets [`Connection`] query methods accept either a bindless `&str` — wrapped
/// as [`sqlx::AssertSqlSafe`] and run through the unprepared text protocol — or a
/// parameterized `sqlx::query(..).bind(..)` value, without callers writing the
/// wrapper at every call site. Callers remain responsible for ensuring bindless
/// strings carry no injection (via read-only validation and identifier quoting).
///
/// The returned [`SqlStr`] owns its text, so no input borrow escapes. The
/// `(SqlStr, Option<_>)` pair is itself an [`sqlx::Execute`] value: a `None`
/// argument set routes through the unprepared text protocol, `Some` through a
/// prepared statement.
pub trait IntoSafeQuery<DB: sqlx::Database> {
    /// Returns the SQL text and the bound arguments, if any.
    ///
    /// # Errors
    ///
    /// [`SqlError::Query`] — extracting bound arguments failed.
    fn into_sql_and_args(self) -> Result<(SqlStr, Option<DB::Arguments>), SqlError>;
}

impl<DB: sqlx::Database> IntoSafeQuery<DB> for &str {
    fn into_sql_and_args(self) -> Result<(SqlStr, Option<DB::Arguments>), SqlError> {
        Ok((AssertSqlSafe(self).into_sql_str(), None))
    }
}

impl<DB: sqlx::Database, A> IntoSafeQuery<DB> for sqlx::query::Query<'_, DB, A>
where
    A: Send + sqlx::IntoArguments<DB>,
{
    fn into_sql_and_args(mut self) -> Result<(SqlStr, Option<DB::Arguments>), SqlError> {
        let arguments = self.take_arguments().map_err(|e| SqlError::Query(e.to_string()))?;
        Ok((self.sql(), arguments))
    }
}

/// Unified query surface every backend tool handler uses.
///
/// Backends supply three required items — [`DB`](Connection::DB),
/// [`pool`](Connection::pool), and [`query_timeout`](Connection::query_timeout)
/// — and receive default implementations for query execution.
///
/// Query methods accept any [`IntoSafeQuery`] value: a bindless `&str` (run
/// through the unprepared text protocol, required for statements like `MySQL`
/// `USE`) or a parameterized `sqlx::query(sql).bind(...)` value (run as a
/// prepared statement).
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
    async fn execute<Q>(&self, query: Q, database: Option<&str>) -> Result<u64, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        let (sql, arguments) = query.into_sql_and_args()?;
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), sql, |sql| async move {
            Ok(pool.execute((sql, arguments)).await?.rows_affected())
        })
        .await
    }

    /// Runs a statement and collects every result row as JSON.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_json<Q>(&self, query: Q, database: Option<&str>) -> Result<Vec<Value>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        let (sql, arguments) = query.into_sql_and_args()?;
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), sql, |sql| async move {
            let rows = pool.fetch_all((sql, arguments)).await?;
            Ok(rows.iter().map(RowExt::to_json).collect())
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
    async fn fetch_optional<Q, T>(&self, query: Q, database: Option<&str>) -> Result<Option<T>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let (sql, arguments) = query.into_sql_and_args()?;
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), sql, |sql| async move {
            let row = pool.fetch_optional((sql, arguments)).await?;
            Ok(row.and_then(|r| r.try_get(0usize).ok()))
        })
        .await
    }

    /// Runs a query and extracts the first column of every row.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_scalar<Q, T>(&self, query: Q, database: Option<&str>) -> Result<Vec<T>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
        T: for<'r> Decode<'r, Self::DB> + Type<Self::DB> + Send + Unpin,
    {
        let (sql, arguments) = query.into_sql_and_args()?;
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), sql, |sql| async move {
            let rows = pool.fetch_all((sql, arguments)).await?;
            rows.iter().map(|r| r.try_get(0usize)).collect()
        })
        .await
    }

    /// Runs a query and decodes every row into `T` via [`sqlx::FromRow`].
    ///
    /// # Errors
    ///
    /// See trait-level documentation. Row decode failures (column type
    /// mismatch, malformed JSON inside a [`sqlx::types::Json`] column, etc.)
    /// surface as [`SqlError::Query`].
    async fn fetch<Q, T>(&self, query: Q, database: Option<&str>) -> Result<Vec<T>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
        T: for<'r> FromRow<'r, <Self::DB as sqlx::Database>::Row> + Send + Unpin,
    {
        let (sql, arguments) = query.into_sql_and_args()?;
        let pool = self.pool(database).await?;
        execute_with_timeout(self.query_timeout(), sql, |sql| async move {
            let rows = pool.fetch_all((sql, arguments)).await?;
            rows.iter().map(T::from_row).collect()
        })
        .await
    }
}
