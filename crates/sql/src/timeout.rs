//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`], the client-side backstop behind each
//! backend's session statement timeout. All backend crates use it instead of
//! duplicating timeout logic.

use std::time::Duration;

use sqlx::{ConnectOptions as _, SqlStr};
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::{Connection, SqlError};

/// Extra time the client backstop waits beyond the server statement timeout.
///
/// Also caps the kill of a timed-out statement.
pub const GRACE: Duration = Duration::from_secs(5);

/// Runs a query operation under the client-side timeout backstop.
///
/// Before `op` runs, [`Connection::arm`] sets the server-side statement
/// timeout on the acquired connection, which normally stops the statement at
/// `connection`'s [`query_timeout`](Connection::query_timeout). This function
/// waits that long + [`GRACE`]; on expiry it drops the query future, spawns a
/// kill of the statement from a one-off connection (capped at [`GRACE`],
/// failures logged), and returns [`SqlError::QueryTimeout`]. A server timeout error
/// ([`Connection::is_timeout`]) also maps to [`SqlError::QueryTimeout`],
/// even when the timeout is `0`; with `0` there is no arm, timer, or kill.
///
/// `op` runs on a connection acquired from `pool`. When the server timeout
/// was not set, the timer stops at the timeout without [`GRACE`]; without a
/// session id, nothing is killed.
///
/// The SQL text is only copied into an owned `String` on the timeout path.
///
/// # Errors
///
/// * [`SqlError::QueryTimeout`] — the server or client-side timeout fired.
/// * [`SqlError::Connection`] — the database could not be reached
///   (I/O, TLS, protocol, or connection pool failure).
/// * [`SqlError::Query`] — the underlying query failed for any other
///   reason (e.g. syntax error).
pub async fn execute_with_timeout<C, T>(
    connection: &C,
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
    let timeout_secs = connection.query_timeout();
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
    let Ok(armed) = tokio::time::timeout_at(deadline, connection.arm(&mut conn)).await else {
        // The arm was dropped mid-flight: keep the connection out of the pool.
        drop(conn.detach());
        return Err(timed_out());
    };
    let (id, armed) = armed.map_err(map_error)?;
    let deadline = if armed { deadline } else { start + limit };

    let Ok(result) = tokio::time::timeout_at(deadline, op(&mut conn, sql)).await else {
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
        return Err(timed_out());
    };
    result.map_err(map_error)
}

#[cfg(test)]
mod tests {
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

    /// How the stub's [`Connection::arm`] behaves.
    #[derive(Clone, Copy)]
    enum Mode {
        /// No server: no session id and no server timeout.
        NoServer,
        /// Reports [`SESSION`] with the server timeout set.
        Server,
        /// Reports [`UNARMED_SESSION`], but the server timeout `SET` failed.
        Unarmed,
        /// The arm never answers.
        SlowArm,
    }

    /// Stub backend with a fixed query timeout and arm behaviour.
    struct Stub {
        mode: Mode,
        timeout_secs: u64,
    }

    impl Stub {
        fn new(mode: Mode, timeout_secs: u64) -> Self {
            Self { mode, timeout_secs }
        }
    }

    impl Connection for Stub {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Err(SqlError::Connection("stub has no pool".into()))
        }

        fn query_timeout(&self) -> u64 {
            self.timeout_secs
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            matches!(error, sqlx::Error::Configuration(m) if m.to_string() == "server timeout")
        }

        async fn arm(&self, _conn: &mut sqlx::SqliteConnection) -> Result<(Option<u64>, bool), sqlx::Error> {
            match self.mode {
                Mode::NoServer => Ok((None, false)),
                Mode::Server => Ok((Some(SESSION), true)),
                Mode::Unarmed => Ok((Some(UNARMED_SESSION), false)),
                Mode::SlowArm => std::future::pending().await,
            }
        }

        fn kill(_conn: &mut sqlx::SqliteConnection, id: u64) -> impl Future<Output = Result<(), sqlx::Error>> + Send {
            KILLED.fetch_or(1 << id, Ordering::SeqCst);
            std::future::ready(Ok(()))
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

    async fn never(_conn: &mut sqlx::SqliteConnection, _sql: SqlStr) -> Result<i32, sqlx::Error> {
        std::future::pending().await
    }

    #[tokio::test]
    async fn server_timeout_maps_to_query_timeout() {
        let pool = pool().await;
        for timeout_secs in [0, 5] {
            let result: Result<i32, SqlError> = execute_with_timeout(
                &Stub::new(Mode::Server, timeout_secs),
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
        let stub = Stub::new(Mode::Server, 5);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(42)).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let pool = pool().await;
        let stub = Stub::new(Mode::Server, 5);
        let result: Result<i32, SqlError> =
            execute_with_timeout(&stub, pool, SqlStr::from_static("BAD SQL"), async |_, _| {
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
            let result: Result<i32, SqlError> = execute_with_timeout(
                &Stub::new(Mode::Server, 0),
                pool.clone(),
                SqlStr::from_static("SELECT 1"),
                async |_, _| Err(error),
            )
            .await;
            let err = result.expect_err("should fail");
            assert!(matches!(err, SqlError::Connection(ref m) if *m == message), "{err:?}");
            assert_eq!(err.to_string(), format!("Database error: {message}"));
        }
    }

    #[tokio::test]
    async fn backstop_fires_after_grace_and_kills_session() {
        let pool = paused_pool().await;
        let stub = Stub::new(Mode::Server, 1);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);
        assert_killed(SESSION).await;
    }

    #[tokio::test]
    async fn unarmed_deadline_has_no_grace_and_kills_session() {
        let pool = paused_pool().await;
        let stub = Stub::new(Mode::Unarmed, 1);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 1.0, 1.5);
        assert_killed(UNARMED_SESSION).await;
    }

    #[tokio::test]
    async fn no_server_deadline_has_no_grace() {
        let pool = paused_pool().await;
        let stub = Stub::new(Mode::NoServer, 1);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 1.0, 1.5);
    }

    #[tokio::test]
    async fn arm_expiry_times_out() {
        let pool = paused_pool().await;
        let stub = Stub::new(Mode::SlowArm, 1);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);
    }

    #[tokio::test]
    async fn acquire_expiry_times_out() {
        let pool = paused_pool().await;
        let _busy = pool.acquire().await.expect("hold the only connection");
        let stub = Stub::new(Mode::Server, 1);
        let result = execute_with_timeout(&stub, pool.clone(), SqlStr::from_static("SELECT slow"), never).await;
        assert_timed_out(result, 6.0, 6.5);
    }

    #[tokio::test]
    async fn huge_timeout_runs_without_timer() {
        let pool = pool().await;
        let stub = Stub::new(Mode::Server, u64::MAX);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(1)).await;
        assert_eq!(result.expect("should succeed"), 1);
    }

    #[tokio::test]
    async fn zero_timeout_disables_limit() {
        let pool = pool().await;
        let stub = Stub::new(Mode::Server, 0);
        let result = execute_with_timeout(&stub, pool, SqlStr::from_static("SELECT 1"), async |_, _| Ok(1)).await;
        assert_eq!(result.expect("should succeed"), 1);
    }
}
