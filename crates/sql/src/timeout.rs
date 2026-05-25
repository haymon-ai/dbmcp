//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`] which runs a query operation under
//! an optional `tokio::time::timeout` guard.  All backend crates use this
//! single function instead of duplicating timeout logic.

use std::time::{Duration, Instant};

use sqlx::SqlStr;

use crate::SqlError;

/// Runs a query operation with an optional query timeout.
///
/// `op` is handed the [`SqlStr`] to execute and returns the in-flight
/// query future. When `timeout_secs` is `Some(n)` with `n > 0`, that
/// future is wrapped with [`tokio::time::timeout`]; on expiry it is
/// dropped (cancelling the in-flight query) and [`SqlError::QueryTimeout`]
/// is returned with the elapsed time and the SQL text. `None` or
/// `Some(0)` runs without a timeout.
///
/// The SQL text is only copied into the error on the timeout path, so the
/// success path adds no allocation.
///
/// # Errors
///
/// * [`SqlError::QueryTimeout`] — the query exceeded the configured
///   timeout.
/// * [`SqlError::Query`] — the underlying query failed for a
///   non-timeout reason (e.g. syntax error, connection loss).
pub async fn execute_with_timeout<T>(
    timeout_secs: Option<u64>,
    sql: SqlStr,
    op: impl AsyncFnOnce(SqlStr) -> Result<T, sqlx::Error>,
) -> Result<T, SqlError> {
    let result = match timeout_secs {
        Some(secs) if secs > 0 => {
            let start = Instant::now();
            let err_sql = sql.clone();
            tokio::time::timeout(Duration::from_secs(secs), op(sql))
                .await
                .map_err(|_| SqlError::QueryTimeout {
                    elapsed_secs: start.elapsed().as_secs_f64(),
                    sql: err_sql.as_str().to_owned(),
                })?
        }
        _ => op(sql).await,
    };
    result.map_err(|e| SqlError::Query(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fast_query_succeeds_with_timeout() {
        let result = execute_with_timeout(Some(5), SqlStr::from_static("SELECT 1"), |_| async { Ok(42) }).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let result: Result<i32, SqlError> = execute_with_timeout(Some(5), SqlStr::from_static("BAD SQL"), |_| async {
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
    async fn slow_query_times_out() {
        let result: Result<i32, SqlError> =
            execute_with_timeout(Some(1), SqlStr::from_static("SELECT SLEEP(60)"), |_| async {
                tokio::time::sleep(Duration::from_mins(1)).await;
                Ok(0)
            })
            .await;
        let err = result.expect_err("should time out");
        match err {
            SqlError::QueryTimeout { elapsed_secs, sql } => {
                assert!(elapsed_secs >= 0.9, "elapsed too small: {elapsed_secs}");
                assert_eq!(sql, "SELECT SLEEP(60)");
            }
            other => panic!("expected QueryTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn none_timeout_runs_without_limit() {
        let result = execute_with_timeout(None, SqlStr::from_static("SELECT 1"), |_| async { Ok(1) }).await;
        assert_eq!(result.expect("should succeed"), 1);
    }

    #[tokio::test]
    async fn zero_timeout_disables_limit() {
        let result = execute_with_timeout(Some(0), SqlStr::from_static("SELECT 1"), |_| async { Ok(1) }).await;
        assert_eq!(result.expect("should succeed"), 1);
    }
}
