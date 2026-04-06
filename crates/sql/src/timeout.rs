//! Query-level timeout wrapper for SQL operations.
//!
//! Provides [`execute_with_timeout`] which wraps any async query future
//! with an optional `tokio::time::timeout` guard.  All backend crates
//! use this single function instead of duplicating timeout logic.

use std::fmt::Display;
use std::time::{Duration, Instant};

use database_mcp_server::AppError;

/// Executes `fut` with an optional query timeout.
///
/// When `timeout_secs` is `Some(n)` where `n > 0`, the future is wrapped
/// with [`tokio::time::timeout`].  On expiry the future is dropped
/// (cancelling the in-flight query) and [`AppError::QueryTimeout`] is
/// returned with the wall-clock elapsed time and the original SQL text.
///
/// When `timeout_secs` is `None` or `Some(0)`, the future runs without
/// any timeout.
///
/// # Errors
///
/// * [`AppError::QueryTimeout`] — the query exceeded the configured
///   timeout.
/// * [`AppError::Query`] — the underlying query failed for a
///   non-timeout reason (e.g. syntax error, connection loss).
pub async fn execute_with_timeout<T, E: Display>(
    timeout_secs: Option<u64>,
    sql: &str,
    fut: impl Future<Output = Result<T, E>>,
) -> Result<T, AppError> {
    match timeout_secs.filter(|&t| t > 0) {
        Some(secs) => {
            let start = Instant::now();
            tokio::time::timeout(Duration::from_secs(secs), fut)
                .await
                .map_err(|_| AppError::QueryTimeout {
                    elapsed_secs: start.elapsed().as_secs_f64(),
                    sql: sql.to_string(),
                })?
                .map_err(|e| AppError::Query(e.to_string()))
        }
        None => fut.await.map_err(|e| AppError::Query(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct TestError(String);

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[tokio::test]
    async fn fast_query_succeeds_with_timeout() {
        let result: Result<i32, AppError> =
            execute_with_timeout(Some(5), "SELECT 1", async { Ok::<_, TestError>(42) }).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn query_error_propagates_as_app_error() {
        let result: Result<i32, AppError> = execute_with_timeout(Some(5), "BAD SQL", async {
            Err::<i32, _>(TestError("syntax error".into()))
        })
        .await;
        let err = result.expect_err("should fail");
        assert!(
            matches!(err, AppError::Query(ref msg) if msg.contains("syntax error")),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn slow_query_times_out() {
        let result: Result<i32, AppError> = execute_with_timeout(Some(1), "SELECT SLEEP(60)", async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<_, TestError>(0)
        })
        .await;
        let err = result.expect_err("should time out");
        match err {
            AppError::QueryTimeout { elapsed_secs, sql } => {
                assert!(elapsed_secs >= 0.9, "elapsed too small: {elapsed_secs}");
                assert_eq!(sql, "SELECT SLEEP(60)");
            }
            other => panic!("expected QueryTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn none_timeout_runs_without_limit() {
        let result: Result<i32, AppError> =
            execute_with_timeout(None, "SELECT 1", async { Ok::<_, TestError>(1) }).await;
        assert_eq!(result.expect("should succeed"), 1);
    }

    #[tokio::test]
    async fn zero_timeout_disables_limit() {
        let result: Result<i32, AppError> =
            execute_with_timeout(Some(0), "SELECT 1", async { Ok::<_, TestError>(1) }).await;
        assert_eq!(result.expect("should succeed"), 1);
    }
}
