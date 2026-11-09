//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`] which runs a query operation under
//! a `tokio::time::timeout` guard (`0` disables it).  All backend crates use this
//! single function instead of duplicating timeout logic.

use std::time::Duration;

use tokio::time::Instant;

use sqlx::SqlStr;

use crate::{Connection, SqlError};

/// Extra time the client timer waits beyond the server statement timeout.
///
/// The server timeout fires first; equal timers race, and a query future
/// dropped while the server's error arrives leaves the connection unusable.
pub const GRACE: Duration = Duration::from_secs(5);

/// Runs a query operation with an optional query timeout.
///
/// `op` is handed the [`SqlStr`] to execute and returns the in-flight
/// query future. When `timeout_secs` is greater than zero, that future is
/// wrapped with [`tokio::time::timeout`] at `timeout_secs` + [`GRACE`],
/// leaving the server-side session timeout room to fire first; on expiry it is dropped
/// (cancelling the in-flight query) and [`SqlError::QueryTimeout`] is
/// returned with the elapsed time and the SQL text. `0` runs without a
/// timeout. A server timeout error ([`Connection::is_timeout`]) also maps
/// to [`SqlError::QueryTimeout`], even when `timeout_secs` is `0`.
///
/// The SQL text is only copied into the error on the timeout path, so the
/// success path adds no allocation.
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
    sql: SqlStr,
    op: impl AsyncFnOnce(SqlStr) -> Result<T, sqlx::Error>,
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
    let result = if timeout_secs > 0 {
        tokio::time::timeout(Duration::from_secs(timeout_secs) + GRACE, op(sql))
            .await
            .map_err(|_| timed_out())?
    } else {
        op(sql).await
    };
    result.map_err(|e| match e {
        e if C::is_timeout(&e) => {
            tracing::debug!(error = %e, "server statement timeout");
            timed_out()
        }
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => SqlError::Connection(e.to_string()),
        e => SqlError::Query(e.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub backend whose server timeout is `Configuration("server timeout")`.
    struct Stub;

    impl Connection for Stub {
        type DB = sqlx::Sqlite;

        async fn pool(&self, _target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
            Err(SqlError::Connection("stub has no pool".into()))
        }

        fn query_timeout(&self) -> u64 {
            0
        }

        fn is_timeout(error: &sqlx::Error) -> bool {
            matches!(error, sqlx::Error::Configuration(m) if m.to_string() == "server timeout")
        }
    }

    fn server_timeout() -> sqlx::Error {
        sqlx::Error::Configuration("server timeout".into())
    }

    #[tokio::test]
    async fn server_timeout_maps_to_query_timeout() {
        for timeout_secs in [0, 5] {
            let result: Result<i32, SqlError> =
                execute_with_timeout::<Stub, _>(timeout_secs, SqlStr::from_static("SELECT slow"), |_| async {
                    Err(server_timeout())
                })
                .await;
            match result.expect_err("should fail") {
                SqlError::QueryTimeout { sql, .. } => assert_eq!(sql, "SELECT slow"),
                other => panic!("expected QueryTimeout under {timeout_secs}, got: {other}"),
            }
        }
    }

    #[tokio::test]
    async fn fast_query_succeeds_with_timeout() {
        let result = execute_with_timeout::<Stub, _>(5, SqlStr::from_static("SELECT 1"), |_| async { Ok(42) }).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let result: Result<i32, SqlError> =
            execute_with_timeout::<Stub, _>(5, SqlStr::from_static("BAD SQL"), |_| async {
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
                execute_with_timeout::<Stub, _>(0, SqlStr::from_static("SELECT 1"), |_| async { Err(error) }).await;
            let err = result.expect_err("should fail");
            assert!(matches!(err, SqlError::Connection(ref m) if *m == message), "{err:?}");
            assert_eq!(err.to_string(), format!("Database error: {message}"));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn slow_query_times_out() {
        let result: Result<i32, SqlError> =
            execute_with_timeout::<Stub, _>(1, SqlStr::from_static("SELECT SLEEP(60)"), |_| async {
                tokio::time::sleep(Duration::from_mins(1)).await;
                Ok(0)
            })
            .await;
        let err = result.expect_err("should time out");
        match err {
            SqlError::QueryTimeout { elapsed_secs, sql } => {
                assert!(elapsed_secs >= 5.9, "elapsed too small: {elapsed_secs}");
                assert_eq!(sql, "SELECT SLEEP(60)");
            }
            other => panic!("expected QueryTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn zero_timeout_disables_limit() {
        let result = execute_with_timeout::<Stub, _>(0, SqlStr::from_static("SELECT 1"), |_| async { Ok(1) }).await;
        assert_eq!(result.expect("should succeed"), 1);
    }
}
