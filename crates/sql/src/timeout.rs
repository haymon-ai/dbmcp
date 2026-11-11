//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`], the client-side backstop behind each
//! backend's session statement timeout. Every [`Connection`] query method
//! runs through it.

use std::time::Duration;

use sqlx::{ConnectOptions as _, SqlStr};
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::connection::IntoSafeQuery;
use crate::{Connection, SqlError};

/// Extra time the client backstop waits beyond the backend statement timeout.
///
/// Also caps the kill of a timed-out statement.
pub(crate) const GRACE: Duration = Duration::from_secs(5);

/// Runs a query operation under the client-side timeout backstop.
///
/// Resolves `connection`'s pool for `database`, acquires a connection, reads
/// its [`session_id`](Connection::session_id), and calls
/// [`set_timeout`](Connection::set_timeout) with the time left of
/// `timeout_secs`, counted from the call's start, before `op` runs `query`'s
/// SQL text and arguments on it. A failed `set_timeout` is logged and the
/// query runs on the client timeout alone. The backstop waits
/// `timeout_secs` + [`GRACE`] (no [`GRACE`] after a failed `set_timeout`);
/// on expiry it drops
/// the query future, spawns a kill of the session (if it has an id) from a
/// one-off connection capped at [`GRACE`], and returns
/// [`SqlError::QueryTimeout`].
/// A backend timeout error ([`Connection::is_timeout`]) also maps to
/// [`SqlError::QueryTimeout`]; a `0` timeout skips the arm, timer, and kill.
///
/// # Errors
///
/// * [`SqlError::InvalidIdentifier`] — `database` failed validation.
/// * [`SqlError::QueryTimeout`] — the backend or client-side timeout fired.
/// * [`SqlError::Connection`] — the database could not be reached
///   (I/O, TLS, protocol, or connection pool failure).
/// * [`SqlError::Query`] — the underlying query failed for any other
///   reason (e.g. syntax error).
pub(crate) async fn execute_with_timeout<C, Q, T>(
    connection: &C,
    database: Option<&str>,
    timeout_secs: u64,
    query: Q,
    op: impl AsyncFnOnce(
        &mut <C::DB as sqlx::Database>::Connection,
        (SqlStr, Option<<C::DB as sqlx::Database>::Arguments>),
    ) -> Result<T, sqlx::Error>,
) -> Result<T, SqlError>
where
    C: Connection + ?Sized,
    Q: IntoSafeQuery<C::DB>,
    for<'c> &'c mut <C::DB as sqlx::Database>::Connection: sqlx::Executor<'c, Database = C::DB>,
    usize: sqlx::ColumnIndex<<C::DB as sqlx::Database>::Row>,
    <C::DB as sqlx::Database>::Row: sqlx_json::RowExt,
    <C::DB as sqlx::Database>::QueryResult: sqlx_json::QueryResult,
{
    let (sql, arguments) = query.into_sql_and_args()?;
    let pool = connection.pool(database).await?;
    let start = Instant::now();
    let err_sql = sql.clone();
    let timed_out = || SqlError::QueryTimeout {
        elapsed_secs: start.elapsed().as_secs_f64(),
        sql: err_sql.as_str().to_owned(),
    };
    let map_error = |e: sqlx::Error| match e {
        e if C::is_timeout(&e) => {
            debug!(error = %e, "backend statement timeout");
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
        return op(&mut conn, (sql, arguments)).await.map_err(map_error);
    };

    let mut conn = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| timed_out())?
        .map_err(map_error)?;
    let Ok(id) = tokio::time::timeout_at(deadline, connection.session_id(&mut conn)).await else {
        // The lookup was dropped mid-flight: keep the connection out of the pool.
        drop(conn.detach());
        return Err(timed_out());
    };
    let id = id.map_err(map_error)?;
    // `deadline` fits in an `Instant`, so `start + limit` does too.
    let query_end = start + limit;
    // At least 1 ms: servers read a `0` timeout as no limit.
    let left = query_end
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(1));
    let armed = match tokio::time::timeout_at(deadline, connection.set_timeout(&mut conn, left)).await {
        Ok(Ok(())) => true,
        // A failed `set_timeout` keeps the statement: only the client timeout applies.
        Ok(Err(error)) => {
            warn!(%error, session_id = id, "statement timeout not set; relying on the client timeout");
            false
        }
        Err(_) => {
            // Dropped mid-flight: a server connection stays out of the pool. One
            // without a session (`SQLite`) is a local handle that survives this;
            // detaching it would let the pool open a second connection.
            if id.is_some() {
                drop(conn.detach());
            }
            return Err(timed_out());
        }
    };
    let query_deadline = if armed { deadline } else { query_end };

    if let Ok(result) = tokio::time::timeout_at(query_deadline, op(&mut conn, (sql, arguments))).await {
        return result.map_err(map_error);
    }
    if let Some(id) = id {
        // `conn` stays out of the pool until the kill ends, so nothing reuses it
        // while the statement may still run. It is then detached: sqlx can't
        // always return a connection whose query future was dropped mid-flight,
        // and would keep its slot forever. After a failed kill the statement may
        // run on, outside `max_connections`, until the server ends it.
        tokio::spawn(async move {
            let kill = async {
                let mut killer = pool.connect_options().connect().await?;
                let result = C::kill(&mut killer, id).await;
                let _ = sqlx::Connection::close(killer).await;
                result
            };
            match tokio::time::timeout(GRACE, kill).await {
                Ok(Ok(())) => debug!(session_id = id, "killed timed-out statement"),
                Ok(Err(error)) => warn!(%error, session_id = id, "failed to kill timed-out statement"),
                Err(_) => warn!(session_id = id, "kill of timed-out statement did not finish in time"),
            }
            drop(conn.detach());
        });
    }
    Err(timed_out())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    /// Session id the [`Mode::Server`] stub reports.
    const SESSION: u64 = 7;

    /// Session id the [`Mode::Unarmed`] stub reports.
    const UNARMED_SESSION: u64 = 8;

    /// Bit `1 << id` is set once [`Stub::kill`] received session `id`.
    static KILLED: AtomicU64 = AtomicU64::new(0);

    fn killed(id: u64) -> bool {
        KILLED.load(Ordering::SeqCst) & (1 << id) != 0
    }

    fn server_timeout() -> sqlx::Error {
        sqlx::Error::Configuration("server timeout".into())
    }

    /// How the stub's [`Connection::session_id`] and [`Connection::set_timeout`] behave.
    #[derive(Clone, Copy)]
    enum Mode {
        /// No session id, so nothing to kill; the timeout is still set.
        NoSession,
        /// Reports [`SESSION`] with the server timeout set.
        Server,
        /// Reports [`UNARMED_SESSION`], but the server timeout `SET` fails.
        Unarmed,
        /// The session id lookup fails.
        FailedLookup,
        /// The session id lookup never answers.
        SlowLookup,
        /// The server timeout `SET` never answers.
        SlowSet,
        /// No session id, and arming the timeout never answers.
        NoSessionSlowSet,
    }

    /// Stub backend with a fixed pool, query timeout, and arm behaviour.
    struct Stub {
        pool: sqlx::SqlitePool,
        mode: Mode,
        timeout_secs: u64,
        set_limit: Mutex<Option<Duration>>,
    }

    impl Stub {
        fn new(pool: &sqlx::SqlitePool, mode: Mode, timeout_secs: u64) -> Self {
            Self {
                pool: pool.clone(),
                mode,
                timeout_secs,
                set_limit: Mutex::new(None),
            }
        }

        /// Returns the limit [`Connection::set_timeout`] received, if it was called.
        fn set_limit(&self) -> Option<Duration> {
            *self.set_limit.lock().expect("set_limit lock")
        }

        fn set_called(&self) -> bool {
            self.set_limit().is_some()
        }
    }

    impl Connection for Stub {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Ok(self.pool.clone())
        }

        fn query_timeout(&self) -> u64 {
            self.timeout_secs
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            matches!(error, sqlx::Error::Configuration(m) if m.to_string() == "server timeout")
        }

        async fn session_id(&self, _conn: &mut sqlx::SqliteConnection) -> Result<Option<u64>, sqlx::Error> {
            match self.mode {
                Mode::NoSession | Mode::NoSessionSlowSet => Ok(None),
                Mode::Server | Mode::SlowSet => Ok(Some(SESSION)),
                Mode::Unarmed => Ok(Some(UNARMED_SESSION)),
                Mode::FailedLookup => Err(sqlx::Error::Configuration("lookup failed".into())),
                Mode::SlowLookup => std::future::pending().await,
            }
        }

        async fn set_timeout(&self, _conn: &mut sqlx::SqliteConnection, limit: Duration) -> Result<(), sqlx::Error> {
            *self.set_limit.lock().expect("set_limit lock") = Some(limit);
            match self.mode {
                Mode::Unarmed => Err(sqlx::Error::Configuration("unknown variable".into())),
                Mode::SlowSet | Mode::NoSessionSlowSet => std::future::pending().await,
                _ => Ok(()),
            }
        }

        async fn kill(_conn: &mut sqlx::SqliteConnection, id: u64) -> Result<(), sqlx::Error> {
            KILLED.fetch_or(1 << id, Ordering::SeqCst);
            Ok(())
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

    /// Resumes tokio time, then polls until the kill task recorded session `id`.
    ///
    /// The kill connects for real, so paused time would auto-advance past its
    /// [`GRACE`] cap before the connect finished.
    async fn assert_killed(id: u64) {
        tokio::time::resume();
        for _ in 0..200 {
            if killed(id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("kill task did not run");
    }

    /// SQL text and arguments `op` receives.
    type Query = (SqlStr, Option<sqlx::sqlite::SqliteArguments>);

    async fn never(_conn: &mut sqlx::SqliteConnection, _query: Query) -> Result<i32, sqlx::Error> {
        std::future::pending().await
    }

    #[tokio::test]
    async fn server_timeout_maps_to_query_timeout() {
        let pool = pool().await;
        for timeout_secs in [0, 5] {
            let result: Result<i32, SqlError> = execute_with_timeout(
                &Stub::new(&pool, Mode::Server, timeout_secs),
                None,
                timeout_secs,
                "SELECT slow",
                async |_, _| Err(server_timeout()),
            )
            .await;
            assert_timed_out(result, 0.0, 1.0);
        }
    }

    #[tokio::test]
    async fn fast_query_succeeds_for_any_timeout() {
        let pool = pool().await;
        // `0` disables the limit; `u64::MAX` overflows an `Instant` and runs without a timer.
        for timeout_secs in [0, 5, u64::MAX] {
            let stub = Stub::new(&pool, Mode::Server, timeout_secs);
            let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT 1", async |_, _| Ok(42)).await;
            assert_eq!(result.expect("should succeed"), 42, "timeout {timeout_secs}");
        }
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let pool = pool().await;
        let stub = Stub::new(&pool, Mode::Server, 5);
        let result: Result<i32, SqlError> =
            execute_with_timeout(&stub, None, stub.timeout_secs, "BAD SQL", async |_, _| {
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
                execute_with_timeout(&Stub::new(&pool, Mode::Server, 0), None, 0, "SELECT 1", async |_, _| {
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
        let stub = Stub::new(&pool, Mode::Server, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
        assert_killed(SESSION).await;
    }

    #[tokio::test]
    async fn unarmed_deadline_has_no_grace_and_kills_session() {
        let pool = paused_pool().await;
        let stub = Stub::new(&pool, Mode::Unarmed, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 1.0, 1.5);
        assert_killed(UNARMED_SESSION).await;
    }

    #[tokio::test]
    async fn no_session_sets_timeout_with_grace() {
        let pool = paused_pool().await;
        let stub = Stub::new(&pool, Mode::NoSession, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
        assert!(stub.set_called(), "set_timeout not called without a session id");
    }

    #[tokio::test]
    async fn failed_lookup_fails_query_without_set() {
        let pool = pool().await;
        let stub = Stub::new(&pool, Mode::FailedLookup, 5);
        let result: Result<i32, SqlError> =
            execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT 1", async |_, _| Ok(42)).await;
        let err = result.expect_err("should fail");
        assert!(
            matches!(err, SqlError::Query(ref msg) if msg.contains("lookup failed")),
            "unexpected error: {err}"
        );
        assert!(!stub.set_called(), "set_timeout called after a failed lookup");
    }

    #[tokio::test]
    async fn lookup_expiry_times_out() {
        let pool = paused_pool().await;
        let stub = Stub::new(&pool, Mode::SlowLookup, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
    }

    #[tokio::test]
    async fn set_expiry_times_out() {
        let pool = paused_pool().await;
        let stub = Stub::new(&pool, Mode::SlowSet, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
        assert_eq!(pool.size(), 0, "server connection must be detached");
    }

    #[tokio::test]
    async fn set_expiry_without_session_keeps_connection_pooled() {
        let pool = paused_pool().await;
        let stub = Stub::new(&pool, Mode::NoSessionSlowSet, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
        assert_eq!(pool.size(), 1, "detaching would let the pool open a second connection");
    }

    #[tokio::test]
    async fn set_timeout_gets_time_left_after_acquire_wait() {
        // Real time: returning the busy connection does I/O that paused time would skip past.
        let pool = pool().await;
        let busy = pool.acquire().await.expect("hold the only connection");
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            drop(busy);
        });
        let stub = Stub::new(&pool, Mode::Server, 2);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT 1", async |_, _| Ok(42)).await;
        assert_eq!(result.expect("should succeed"), 42);
        let limit = stub.set_limit().expect("set_timeout called");
        assert!(
            (Duration::from_secs(1)..=Duration::from_millis(1500)).contains(&limit),
            "limit {limit:?} should be the 1.5 s left after the 0.5 s wait"
        );
    }

    #[tokio::test]
    async fn acquire_expiry_times_out() {
        let pool = paused_pool().await;
        let _busy = pool.acquire().await.expect("hold the only connection");
        let stub = Stub::new(&pool, Mode::Server, 1);
        let result = execute_with_timeout(&stub, None, stub.timeout_secs, "SELECT slow", never).await;
        assert_timed_out(result, 6.0, 6.5);
    }
}
