//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single trait every backend implements.
//! Backends provide pool resolution, timeout config, and timeout hooks;
//! default method implementations handle query execution.

use crate::SqlError;
use futures_util::TryStreamExt as _;
use serde_json::Value;
use sqlx::{AssertSqlSafe, Decode, Either, Execute, Executor, FromRow, Row, SqlSafeStr, SqlStr, Type};
use sqlx_json::{QueryResult, RowExt};

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
/// Backends supply the [`DB`](Connection::DB) type, [`pool`](Connection::pool),
/// [`query_timeout`](Connection::query_timeout), and the timeout hooks
/// [`is_timeout`](Connection::is_timeout), [`session_id`](Connection::session_id),
/// [`set_timeout`](Connection::set_timeout), and [`kill`](Connection::kill),
/// and receive default implementations for query execution. They may
/// override [`counts`](Connection::counts) when their query results do
/// not describe their own statement.
///
/// Query methods accept any [`IntoSafeQuery`] value: a bindless `&str` (run
/// through the unprepared text protocol, which some `MySQL` statements need)
/// or a parameterized `sqlx::query(sql).bind(...)` value (run as a
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
pub trait Connection: Send + Sync + 'static
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

    /// Returns the configured query timeout in seconds (`0` = disabled).
    fn query_timeout(&self) -> u64;

    /// Returns whether `error` is this backend's server-side statement timeout.
    fn is_timeout(error: &sqlx::Error) -> bool;

    /// Reads the server session id of `conn`; `None` means no server.
    ///
    /// Without a session id, no server statement timeout is set and a
    /// timed-out statement is not killed.
    ///
    /// # Errors
    ///
    /// Returns the driver error if the lookup fails.
    async fn session_id(&self, conn: &mut <Self::DB as sqlx::Database>::Connection)
    -> Result<Option<u64>, sqlx::Error>;

    /// Sets the server statement timeout on `conn` to `secs` seconds.
    ///
    /// Called only when [`session_id`](Connection::session_id) returned an id.
    ///
    /// # Errors
    ///
    /// Returns the driver error if the server rejects the timeout.
    async fn set_timeout(
        &self,
        conn: &mut <Self::DB as sqlx::Database>::Connection,
        secs: u64,
    ) -> Result<(), sqlx::Error>;

    /// Cancels the statement running on session `id`, sent over a one-off `conn`.
    ///
    /// # Errors
    ///
    /// Returns the driver error if cancelling fails.
    fn kill(
        conn: &mut <Self::DB as sqlx::Database>::Connection,
        id: u64,
    ) -> impl Future<Output = Result<(), sqlx::Error>> + Send;

    /// Checks that the default database answers `SELECT 1`.
    ///
    /// Succeeds without a query while every pooled connection is busy:
    /// a ping would otherwise queue behind long-running queries and fail
    /// a healthy server. How often and how long callers ping (rate limit,
    /// deadline, shutdown) is the caller's policy, not this method's.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn ping(&self) -> Result<(), SqlError> {
        let pool = self.pool(None).await?;
        if pool.size() > 0 && pool.num_idle() == 0 {
            return Ok(());
        }
        self.execute("SELECT 1", None).await.map(|_| ())
    }

    /// Runs a statement that returns no meaningful rows.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn execute<Q>(&self, query: Q, database: Option<&str>) -> Result<u64, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        execute_with_timeout(self, database, self.query_timeout(), query, async |conn, query| {
            Ok(conn.execute(query).await?.rows_affected())
        })
        .await
    }

    /// Runs a statement under `timeout_secs` and collects every result row as JSON.
    ///
    /// `timeout_secs` replaces [`query_timeout`](Connection::query_timeout)
    /// for this call (`0` = disabled).
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn fetch_json<Q>(&self, query: Q, database: Option<&str>, timeout_secs: u64) -> Result<Vec<Value>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        execute_with_timeout(self, database, timeout_secs, query, async |conn, query| {
            let rows = conn.fetch_all(query).await?;
            Ok(rows.iter().map(RowExt::to_json).collect())
        })
        .await
    }

    /// Runs a statement and returns its rows as JSON, affected rows, and insert id.
    ///
    /// One execution yields both the returned rows (e.g. `RETURNING`) and
    /// the counts, which [`counts`](Connection::counts) folds from the
    /// per-statement results.
    ///
    /// # Errors
    ///
    /// See trait-level documentation.
    async fn execute_json<Q>(
        &self,
        query: Q,
        database: Option<&str>,
    ) -> Result<(Vec<Value>, u64, Option<u64>), SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        execute_with_timeout(self, database, self.query_timeout(), query, async |conn, query| {
            let sql = query.0.clone();
            let mut stream = conn.fetch_many(query);
            let mut rows = Vec::new();
            let mut results = Vec::new();
            while let Some(item) = stream.try_next().await? {
                match item {
                    Either::Left(result) => results.push(result),
                    Either::Right(row) => rows.push(row.to_json()),
                }
            }
            let (rows_affected, last_insert_id) = Self::counts(sql.as_str(), &results);
            Ok((rows, rows_affected, last_insert_id))
        })
        .await
    }

    /// Folds per-statement results of `sql` into affected rows and the last insert id.
    ///
    /// The default sums [`QueryResult::rows_affected`] and keeps the last
    /// [`QueryResult::last_insert_id`]. Backends whose results do not
    /// describe their own statement override it.
    fn counts(_sql: &str, results: &[<Self::DB as sqlx::Database>::QueryResult]) -> (u64, Option<u64>) {
        let rows_affected = results.iter().map(QueryResult::rows_affected).sum();
        let last_insert_id = results.iter().rev().find_map(QueryResult::last_insert_id);
        (rows_affected, last_insert_id)
    }

    /// Runs a statement in a read-only transaction and collects every row as JSON.
    ///
    /// The database itself rejects writes, so side-effecting functions that
    /// pass read-only validation (sequences, large objects, user-defined
    /// functions) cannot modify data. The transaction is always rolled back,
    /// which on `PostgreSQL` also discards session settings changed by the
    /// statement; `MySQL` user variables persist.
    ///
    /// Requires `START TRANSACTION READ ONLY` support (`MySQL`, `MariaDB`,
    /// `PostgreSQL`); `SQLite` rejects it.
    ///
    /// `timeout_secs` replaces [`query_timeout`](Connection::query_timeout)
    /// for this call (`0` = disabled).
    ///
    /// # Errors
    ///
    /// See trait-level documentation. Writes attempted by the statement
    /// surface as [`SqlError::Query`].
    async fn fetch_json_read_only<Q>(
        &self,
        query: Q,
        database: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Vec<Value>, SqlError>
    where
        Q: IntoSafeQuery<Self::DB>,
    {
        execute_with_timeout(self, database, timeout_secs, query, async |conn, query| {
            let mut tx = sqlx::Connection::begin_with(conn, SqlStr::from_static("START TRANSACTION READ ONLY")).await?;
            let rows = (&mut *tx).fetch_all(query).await?;
            tx.rollback().await?;
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
        execute_with_timeout(self, database, self.query_timeout(), query, async |conn, query| {
            let row = conn.fetch_optional(query).await?;
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
        execute_with_timeout(self, database, self.query_timeout(), query, async |conn, query| {
            let rows = conn.fetch_all(query).await?;
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
        execute_with_timeout(self, database, self.query_timeout(), query, async |conn, query| {
            let rows = conn.fetch_all(query).await?;
            rows.iter().map(T::from_row).collect()
        })
        .await
    }
}
