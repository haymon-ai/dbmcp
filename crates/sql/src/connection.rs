//! Connection abstraction shared across database backends.
//!
//! Defines [`Connection`] — the single entry point every backend tool
//! handler uses to run SQL — plus the sealed [`Query`] and [`Executable`]
//! input traits that enumerate the accepted statement shapes.

use database_mcp_server::AppError;
use sqlx::Database;
use sqlx::pool::PoolConnection;

mod private {
    pub trait Sealed {}
}

/// Shapes accepted by [`Connection::fetch`] and [`Connection::fetch_optional`].
///
/// Sealed — only the in-crate implementations for [`sqlx::query::Query`],
/// [`sqlx::query::QueryAs`], [`sqlx::query::QueryScalar`], and `&str` are
/// permitted.
pub trait Query<DB>: private::Sealed + Send
where
    DB: Database,
{
    /// Decoded per-row output type.
    type Output: Send + Unpin;

    /// SQL text, captured before running so timeout errors can carry it.
    fn sql(&self) -> &str;

    /// Runs the query against an acquired pool connection and collects every row.
    #[doc(hidden)]
    fn run_fetch_all(
        self,
        conn: &mut PoolConnection<DB>,
    ) -> impl Future<Output = Result<Vec<Self::Output>, sqlx::Error>> + Send;

    /// Runs the query against an acquired pool connection and returns at most one row.
    #[doc(hidden)]
    fn run_fetch_optional(
        self,
        conn: &mut PoolConnection<DB>,
    ) -> impl Future<Output = Result<Option<Self::Output>, sqlx::Error>> + Send;
}

/// Shapes accepted by [`Connection::execute`].
///
/// Only untyped statements make sense to `execute` — this trait is sealed
/// and implemented exclusively by [`sqlx::query::Query`] and `&str`.
pub trait Executable<DB>: Query<DB>
where
    DB: Database,
{
    /// Runs the statement against an acquired pool connection and discards any rows.
    #[doc(hidden)]
    fn run_execute(
        self,
        conn: &mut PoolConnection<DB>,
    ) -> impl Future<Output = Result<DB::QueryResult, sqlx::Error>> + Send;
}

// ────────────────────── sqlx::query::Query ──────────────────────

impl<'q, DB> private::Sealed for sqlx::query::Query<'q, DB, <DB as Database>::Arguments<'q>> where DB: Database {}

impl<'q, DB> Query<DB> for sqlx::query::Query<'q, DB, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    <DB as Database>::Arguments<'q>: Send + sqlx::IntoArguments<'q, DB>,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    type Output = <DB as Database>::Row;

    fn sql(&self) -> &str {
        sqlx::Execute::sql(self)
    }

    async fn run_fetch_all(self, conn: &mut PoolConnection<DB>) -> Result<Vec<<DB as Database>::Row>, sqlx::Error> {
        sqlx::query::Query::fetch_all(self, &mut **conn).await
    }

    async fn run_fetch_optional(
        self,
        conn: &mut PoolConnection<DB>,
    ) -> Result<Option<<DB as Database>::Row>, sqlx::Error> {
        sqlx::query::Query::fetch_optional(self, &mut **conn).await
    }
}

impl<'q, DB> Executable<DB> for sqlx::query::Query<'q, DB, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    <DB as Database>::Arguments<'q>: Send + sqlx::IntoArguments<'q, DB>,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    async fn run_execute(self, conn: &mut PoolConnection<DB>) -> Result<<DB as Database>::QueryResult, sqlx::Error> {
        sqlx::query::Query::execute(self, &mut **conn).await
    }
}

// ────────────────────── sqlx::query::QueryAs ──────────────────────

impl<'q, DB, T> private::Sealed for sqlx::query::QueryAs<'q, DB, T, <DB as Database>::Arguments<'q>> where DB: Database {}

impl<'q, DB, T> Query<DB> for sqlx::query::QueryAs<'q, DB, T, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    T: Send + Unpin + for<'r> sqlx::FromRow<'r, <DB as Database>::Row> + 'q,
    <DB as Database>::Arguments<'q>: Send + sqlx::IntoArguments<'q, DB>,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    type Output = T;

    fn sql(&self) -> &str {
        sqlx::Execute::sql(self)
    }

    async fn run_fetch_all(self, conn: &mut PoolConnection<DB>) -> Result<Vec<T>, sqlx::Error> {
        sqlx::query::QueryAs::fetch_all(self, &mut **conn).await
    }

    async fn run_fetch_optional(self, conn: &mut PoolConnection<DB>) -> Result<Option<T>, sqlx::Error> {
        sqlx::query::QueryAs::fetch_optional(self, &mut **conn).await
    }
}

// ────────────────────── sqlx::query::QueryScalar ──────────────────────

impl<'q, DB, O> private::Sealed for sqlx::query::QueryScalar<'q, DB, O, <DB as Database>::Arguments<'q>> where
    DB: Database
{
}

impl<'q, DB, O> Query<DB> for sqlx::query::QueryScalar<'q, DB, O, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    O: Send + Unpin + 'q,
    (O,): for<'r> sqlx::FromRow<'r, <DB as Database>::Row>,
    <DB as Database>::Arguments<'q>: Send + sqlx::IntoArguments<'q, DB>,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    type Output = O;

    fn sql(&self) -> &str {
        sqlx::Execute::sql(self)
    }

    async fn run_fetch_all(self, conn: &mut PoolConnection<DB>) -> Result<Vec<O>, sqlx::Error> {
        sqlx::query::QueryScalar::fetch_all(self, &mut **conn).await
    }

    async fn run_fetch_optional(self, conn: &mut PoolConnection<DB>) -> Result<Option<O>, sqlx::Error> {
        sqlx::query::QueryScalar::fetch_optional(self, &mut **conn).await
    }
}

// ────────────────────── &str ──────────────────────

impl private::Sealed for &str {}

impl<DB> Query<DB> for &str
where
    DB: Database,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    type Output = <DB as Database>::Row;

    fn sql(&self) -> &str {
        self
    }

    async fn run_fetch_all(self, conn: &mut PoolConnection<DB>) -> Result<Vec<<DB as Database>::Row>, sqlx::Error> {
        use sqlx::Executor;
        (&mut **conn).fetch_all(self).await
    }

    async fn run_fetch_optional(
        self,
        conn: &mut PoolConnection<DB>,
    ) -> Result<Option<<DB as Database>::Row>, sqlx::Error> {
        use sqlx::Executor;
        (&mut **conn).fetch_optional(self).await
    }
}

impl<DB> Executable<DB> for &str
where
    DB: Database,
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
{
    async fn run_execute(self, conn: &mut PoolConnection<DB>) -> Result<<DB as Database>::QueryResult, sqlx::Error> {
        use sqlx::Executor;
        (&mut **conn).execute(self).await
    }
}

// ────────────────────── Connection trait ──────────────────────

/// Hands out the unified query surface every backend tool handler uses.
///
/// Three methods mirror sqlx's `Executor` semantics: [`execute`](Connection::execute),
/// [`fetch`](Connection::fetch), and [`fetch_optional`](Connection::fetch_optional).
/// Access control is delegated entirely to the database server's own credentials.
pub trait Connection: Send + Sync {
    /// Backend sqlx database marker (`Postgres`, `MySql`, or `Sqlite`).
    type Database: Database;

    /// Runs a statement that returns no rows.
    ///
    /// # Errors
    ///
    /// - [`AppError::InvalidIdentifier`] — `target` failed identifier validation.
    /// - [`AppError::PoolCacheFull`] — (Postgres only) pool cache is full and cannot evict.
    /// - [`AppError::Connection`] — the underlying driver failed.
    fn execute<Q>(
        &self,
        query: Q,
        target: Option<&str>,
    ) -> impl Future<Output = Result<<Self::Database as Database>::QueryResult, AppError>> + Send
    where
        Q: Executable<Self::Database>;

    /// Runs a statement and collects every result row or decoded value.
    ///
    /// # Errors
    ///
    /// See [`execute`](Connection::execute).
    fn fetch<Q>(&self, query: Q, target: Option<&str>) -> impl Future<Output = Result<Vec<Q::Output>, AppError>> + Send
    where
        Q: Query<Self::Database>;

    /// Runs a statement and returns at most one result row or decoded value.
    ///
    /// # Errors
    ///
    /// See [`execute`](Connection::execute).
    fn fetch_optional<Q>(
        &self,
        query: Q,
        target: Option<&str>,
    ) -> impl Future<Output = Result<Option<Q::Output>, AppError>> + Send
    where
        Q: Query<Self::Database>;
}
