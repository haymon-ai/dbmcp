//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`], the client-side backstop behind each
//! backend's session statement timeout. All backend crates use it instead of
//! duplicating timeout logic.

use std::time::Duration;

use sqlx::SqlStr;
use sqlx::pool::PoolConnection;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::{Connection, SqlError};

/// Extra time the client backstop waits beyond the server statement timeout.
///
/// Also caps the kill of a timed-out statement.
pub const GRACE: Duration = Duration::from_secs(5);

/// Runs a query operation under the client-side timeout backstop.
///
/// The server-side session timeout (set per connection by each backend)
/// normally stops the statement at `timeout_secs`. This function waits
/// `timeout_secs` + 5 s; on expiry it drops the query future, spawns a kill
/// of the statement from a one-off connection (capped at 5 s, failures
/// logged), and returns [`SqlError::QueryTimeout`]. A server timeout error
/// ([`Connection::is_timeout`]) also maps to [`SqlError::QueryTimeout`],
/// even when `timeout_secs` is `0`; with `0` there is no timer and no kill.
///
/// `op` runs on a connection acquired from `pool`. A backend without a
/// session id ([`Connection::session_id`] returns `None`) has no server
/// timeout, so its timer stops at `timeout_secs` and nothing is killed.
///
/// The SQL text is only copied into the error on the timeout path.
///
/// # Errors
///
/// * [`SqlError::QueryTimeout`] — the server or client-side timeout fired.
/// * [`SqlError::Connection`] — the database could not be reached
///   (I/O, TLS, protocol, or connection pool failure).
/// * [`SqlError::Query`] — the underlying query failed for any other
///   reason (e.g. syntax error).
pub async fn execute_with_timeout<C, T>(
    timeout_secs: u64,
    pool: sqlx::Pool<C::DB>,
    sql: SqlStr,
    op: impl AsyncFnOnce(&mut <C::DB as sqlx::Database>::Connection, SqlStr) -> Result<T, sqlx::Error>,
) -> Result<T, SqlError>
where
    C: Connection + ?Sized,
    for<'c> &'c mut <C::DB as sqlx::Database>::Connection: sqlx::Executor<'c, Database = C::DB>,
    usize: sqlx::ColumnIndex<<C::DB as sqlx::Database>::Row>,
    <C::DB as sqlx::Database>::Row: sqlx_json::RowExt,
    <C::DB as sqlx::Database>::QueryResult: sqlx_json::QueryResult,
{
    let start = Instant::now();
    let err_sql = sql.clone();
    let timed_out = || SqlError::QueryTimeout {
        elapsed_secs: start.elapsed().as_secs_f64(),
        sql: err_sql.as_str().to_owned(),
    };
    let map_error = |e: sqlx::Error| match e {
        e if C::is_timeout(&e) => {
            debug!(error = %e, "server statement timeout");
            timed_out()
        }
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => SqlError::Connection(e.to_string()),
        e => SqlError::Query(e.to_string()),
    };

    let limit = Duration::from_secs(timeout_secs);
    // A timeout too large for an `Instant` never fires: treat it like `0`.
    let backstop = start.checked_add(limit.saturating_add(GRACE));
    let Some(deadline) = backstop.filter(|_| timeout_secs > 0) else {
        let mut conn = pool.acquire().await.map_err(map_error)?;
        return op(&mut conn, sql).await.map_err(map_error);
    };

    let mut conn = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| timed_out())?
        .map_err(map_error)?;
    let Ok(id) = tokio::time::timeout_at(deadline, C::session_id(&mut conn)).await else {
        // The lookup was dropped mid-flight: keep the connection out of the pool.
        drop(conn.detach());
        return Err(timed_out());
    };
    let id = id.map_err(map_error)?;
    let deadline = if id.is_some() { deadline } else { start + limit };

    let Ok(result) = tokio::time::timeout_at(deadline, op(&mut conn, sql)).await else {
        if let Some(id) = id {
            tokio::spawn(kill_statement::<C>(pool, conn, id));
        }
        return Err(timed_out());
    };
    result.map_err(map_error)
}

/// Kills the statement on session `id`, holding `conn` until the kill ends.
///
/// Keeping `conn` out of the pool means no other call can reuse it while
/// the timed-out statement may still run. After a finished kill, `conn` is
/// detached and dropped: sqlx can't always return a connection whose query
/// future was dropped mid-flight, and would keep its pool slot forever.
async fn kill_statement<C>(pool: sqlx::Pool<C::DB>, conn: PoolConnection<C::DB>, id: u64)
where
    C: Connection + ?Sized,
    for<'c> &'c mut <C::DB as sqlx::Database>::Connection: sqlx::Executor<'c, Database = C::DB>,
    usize: sqlx::ColumnIndex<<C::DB as sqlx::Database>::Row>,
    <C::DB as sqlx::Database>::Row: sqlx_json::RowExt,
    <C::DB as sqlx::Database>::QueryResult: sqlx_json::QueryResult,
{
    match tokio::time::timeout(GRACE, C::kill(pool, id)).await {
        Ok(Ok(running)) => {
            debug!(session_id = id, running, "killed timed-out statement");
            drop(conn.detach());
        }
        Ok(Err(error)) => warn!(%error, session_id = id, "failed to kill timed-out statement"),
        Err(_) => warn!(session_id = id, "kill of timed-out statement did not finish in time"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    /// Session id the [`Server`] stub reports.
    const SESSION: u64 = 7;

    /// Session id the last [`Server::kill`] call received (`0` = none).
    static KILLED: AtomicU64 = AtomicU64::new(0);

    fn server_timeout() -> sqlx::Error {
        sqlx::Error::Configuration("server timeout".into())
    }

    fn is_server_timeout(error: &sqlx::Error) -> bool {
        matches!(error, sqlx::Error::Configuration(m) if m.to_string() == "server timeout")
    }

    /// Stub backend without a server: no session id, nothing to kill.
    struct NoServer;

    impl Connection for NoServer {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Err(SqlError::Connection("stub has no pool".into()))
        }

        fn query_timeout(&self) -> u64 {
            0
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            is_server_timeout(error)
        }

        async fn session_id(_conn: &mut sqlx::SqliteConnection) -> Result<Option<u64>, sqlx::Error> {
            Ok(None)
        }

        fn kill(_pool: sqlx::SqlitePool, _id: u64) -> impl Future<Output = Result<bool, sqlx::Error>> + Send {
            std::future::ready(Ok(false))
        }
    }

    /// Stub server backend: reports [`SESSION`] and records kills in [`KILLED`].
    struct Server;

    impl Connection for Server {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Err(SqlError::Connection("stub has no pool".into()))
        }

        fn query_timeout(&self) -> u64 {
            0
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            is_server_timeout(error)
        }

        async fn session_id(_conn: &mut sqlx::SqliteConnection) -> Result<Option<u64>, sqlx::Error> {
            Ok(Some(SESSION))
        }

        fn kill(_pool: sqlx::SqlitePool, id: u64) -> impl Future<Output = Result<bool, sqlx::Error>> + Send {
            KILLED.store(id, Ordering::SeqCst);
            std::future::ready(Ok(true))
        }
    }

    /// Stub server backend whose session id lookup never answers.
    struct SlowId;

    impl Connection for SlowId {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Err(SqlError::Connection("stub has no pool".into()))
        }

        fn query_timeout(&self) -> u64 {
            0
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            is_server_timeout(error)
        }

        async fn session_id(_conn: &mut sqlx::SqliteConnection) -> Result<Option<u64>, sqlx::Error> {
            std::future::pending().await
        }

        fn kill(_pool: sqlx::SqlitePool, _id: u64) -> impl Future<Output = Result<bool, sqlx::Error>> + Send {
            std::future::ready(Ok(false))
        }
    }

    /// Opens a one-connection in-memory pool.
    async fn pool() -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .test_before_acquire(false)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool")
    }

    /// Opens a one-connection in-memory pool, then pauses tokio time.
    ///
    /// The connection is idle before time stops, so the single acquire under
    /// test needs no I/O that auto-advancing time could race.
    async fn paused_pool() -> sqlx::SqlitePool {
        let pool = pool().await;
        while pool.num_idle() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::pause();
        pool
    }

    fn assert_timed_out(result: Result<i32, SqlError>, min_secs: f64, max_secs: f64) {
        match result.expect_err("should time out") {
            SqlError::QueryTimeout { elapsed_secs, sql } => {
                assert!(
                    (min_secs..max_secs).contains(&elapsed_secs),
                    "elapsed {elapsed_secs} outside {min_secs}..{max_secs}"
                );
                assert_eq!(sql, "SELECT slow");
            }
            other => panic!("expected QueryTimeout, got: {other}"),
        }
    }

    async fn never(_conn: &mut sqlx::SqliteConnection, _sql: SqlStr) -> Result<i32, sqlx::Error> {
        std::future::pending().await
    }

    #[tokio::test]
    async fn server_timeout_maps_to_query_timeout() {
        let pool = pool().await;
        for timeout_secs in [0, 5] {
            let result: Result<i32, SqlError> = execute_with_timeout::<Server, _>(
                timeout_secs,
                pool.clone(),
                SqlStr::from_static("SELECT slow"),
                async |_, _| Err(server_timeout()),
            )
            .await;
            assert_timed_out(result, 0.0, 1.0);
        }
    }

    #[tokio::test]
    async fn fast_query_succeeds_with_timeout() {
        let pool = pool().await;
        let result =
            execute_with_timeout::<Server, _>(5, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(42)).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let pool = pool().await;
        let result: Result<i32, SqlError> =
            execute_with_timeout::<Server, _>(5, pool, SqlStr::from_static("BAD SQL"), async |_, _| {
                Err(sqlx::Error::Configuration("syntax error".into()))
            })
            .await;
        let err = result.expect_err("should fail");
        assert!(
            matches!(err, SqlError::Query(ref msg) if msg.contains("syntax error")),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn connectivity_errors_become_connection_errors() {
        let pool = pool().await;
        for error in [
            sqlx::Error::Io(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset")),
            sqlx::Error::Tls("bad certificate".into()),
            sqlx::Error::Protocol("unexpected eof".into()),
            sqlx::Error::PoolTimedOut,
            sqlx::Error::PoolClosed,
            sqlx::Error::WorkerCrashed,
        ] {
            let message = error.to_string();
            let result: Result<i32, SqlError> =
                execute_with_timeout::<Server, _>(0, pool.clone(), SqlStr::from_static("SELECT 1"), async |_, _| {
                    Err(error)
                })
                .await;
            let err = result.expect_err("should fail");
            assert!(matches!(err, SqlError::Connection(ref m) if *m == message), "{err:?}");
            assert_eq!(err.to_string(), format!("Database error: {message}"));
        }
    }

    #[tokio::test]
    async fn backstop_fires_after_grace_and_kills_session() {
        let pool = paused_pool().await;
        let result =
            execute_with_timeout::<Server, _>(1, pool.clone(), SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);

        for _ in 0..100 {
            if KILLED.load(Ordering::SeqCst) == SESSION {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("kill task did not run");
    }

    #[tokio::test]
    async fn no_server_deadline_has_no_grace() {
        let pool = paused_pool().await;
        let result = execute_with_timeout::<NoServer, _>(1, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 1.0, 1.5);
    }

    #[tokio::test]
    async fn session_id_lookup_expiry_times_out() {
        let pool = paused_pool().await;
        let result = execute_with_timeout::<SlowId, _>(1, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);
    }

    #[tokio::test]
    async fn acquire_expiry_times_out() {
        let pool = paused_pool().await;
        let _busy = pool.acquire().await.expect("hold the only connection");
        let result =
            execute_with_timeout::<Server, _>(1, pool.clone(), SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);
    }

    #[tokio::test]
    async fn huge_timeout_runs_without_timer() {
        let pool = pool().await;
        let result =
            execute_with_timeout::<Server, _>(u64::MAX, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(1))
                .await;
        assert_eq!(result.expect("should succeed"), 1);
    }

    #[tokio::test]
    async fn zero_timeout_disables_limit() {
        let pool = pool().await;
        let result =
            execute_with_timeout::<Server, _>(0, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(1)).await;
        assert_eq!(result.expect("should succeed"), 1);
    }
}
