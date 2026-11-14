//! `SQLite` connection: pool ownership, initialization, and [`Connection`] impl.
//!
//! Owns the single lazy [`SqlitePool`] used by [`SqliteHandler`](crate::SqliteHandler).
//! `SQLite` is a single-file, single-writer backend; the pool is fixed
//! at one connection. In read-only mode the file is opened with
//! `SQLITE_OPEN_READONLY`, so `SQLite` itself rejects every write.
//! A progress handler interrupts statements that outlive the query timeout.
//! It also holds the catalog cache next to the pool.

use std::sync::Arc;
use std::time::Duration;

use dbmcp_config::{CacheConfig, DatabaseConfig};
use dbmcp_sql::Connection;
use dbmcp_sql::SqlError;
use moka::future::Cache;
use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use sqlx::sqlite::{SqliteArguments, SqliteConnectOptions, SqlitePool, SqliteQueryResult, SqliteRow};
use sqlx::{Executor as _, SqlStr};
use tracing::info;

use crate::tools::Snapshot;

/// Virtual machine instructions `SQLite` runs between deadline checks.
const PROGRESS_OPS: i32 = 1000;

/// Owns the lazy `SQLite` pool, the logic that builds it, and the catalog cache.
#[derive(Clone)]
pub(crate) struct SqliteConnection {
    config: DatabaseConfig,
    pool: SqlitePool,
    /// Catalog snapshot under one constant key; `None` when caching is off.
    pub(crate) catalog: Option<Cache<(), Arc<Snapshot>>>,
}

impl std::fmt::Debug for SqliteConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteConnection").finish_non_exhaustive()
    }
}

impl SqliteConnection {
    /// Builds the connection, its lazy pool, and the catalog cache when `cache` is set.
    pub(crate) fn new(config: &DatabaseConfig, cache: Option<CacheConfig>) -> Self {
        info!("SQLite lazy connection pool created");

        Self {
            config: config.clone(),
            pool: create_lazy_pool(config),
            catalog: cache.map(|cache| Cache::builder().time_to_live(cache.ttl).build()),
        }
    }

    /// Returns the single pool. Target is ignored (`SQLite` is single-file).
    ///
    /// Crate-private so every tool path goes through the unified
    /// [`Connection`] methods and cannot bypass timeout / error capture.
    #[allow(clippy::unused_async)]
    pub(crate) async fn pool(&self, _target: Option<&str>) -> Result<SqlitePool, SqlError> {
        Ok(self.pool.clone())
    }

    /// Closes the single pool, awaiting a clean shutdown.
    ///
    /// Idempotent — `SqlitePool::close` is safe to call more than once.
    pub(crate) async fn shutdown(&self) {
        self.pool.close().await;
    }
}

impl Connection for SqliteConnection {
    type DB = sqlx::Sqlite;

    async fn pool(&self, target: Option<&str>) -> Result<sqlx::Pool<Self::DB>, SqlError> {
        self.pool(target).await
    }

    fn query_timeout(&self) -> u64 {
        self.config.query_timeout
    }

    fn read_only(&self) -> bool {
        self.config.read_only
    }

    fn is_timeout(error: &sqlx::Error) -> bool {
        // Only the progress handler set by `set_timeout` interrupts statements.
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .is_some_and(|code| code == "9") // SQLITE_INTERRUPT
    }

    /// Counts only the statements that set the change count and rowid.
    ///
    /// `SQLite` keeps both per connection, so other statements still report
    /// earlier values. Unparsable SQL reports `0`.
    fn counts(sql: &str, results: &[SqliteQueryResult]) -> (u64, Option<u64>) {
        let statements = Parser::parse_sql(&SQLiteDialect {}, sql).unwrap_or_default();
        let mut rows_affected = 0;
        let mut last_insert_id = None;
        for (statement, result) in statements.iter().zip(results) {
            match statement {
                Statement::Insert(_) if result.rows_affected() > 0 => {
                    rows_affected += result.rows_affected();
                    last_insert_id = u64::try_from(result.last_insert_rowid()).ok();
                }
                Statement::Update(_) | Statement::Delete(_) => rows_affected += result.rows_affected(),
                _ => {}
            }
        }
        (rows_affected, last_insert_id)
    }

    /// Fetches under `PRAGMA query_only`: `SQLite` has no read-only transactions.
    ///
    /// A file opened read-only needs no pragma. Otherwise the pool's
    /// `before_acquire` hook switches `query_only` off again before the
    /// connection's next use.
    async fn fetch_read_only(
        &self,
        conn: &mut sqlx::SqliteConnection,
        query: (SqlStr, Option<SqliteArguments>),
    ) -> Result<Vec<SqliteRow>, sqlx::Error> {
        if !self.read_only() {
            sqlx::query("PRAGMA query_only = ON").execute(&mut *conn).await?;
        }
        conn.fetch_all(query).await
    }

    async fn session_id(&self, _conn: &mut sqlx::SqliteConnection) -> Result<Option<u64>, sqlx::Error> {
        Ok(None)
    }

    /// Installs a progress handler that interrupts statements after `limit`.
    ///
    /// Reinstalled before every statement, replacing the previous deadline.
    /// The handler stays on the connection afterwards, so every statement
    /// must run through the timeout wrapper. It runs on the driver's worker
    /// thread, so it reads the system clock rather than tokio time.
    async fn set_timeout(&self, conn: &mut sqlx::SqliteConnection, limit: Duration) -> Result<(), sqlx::Error> {
        let start = std::time::Instant::now();
        conn.lock_handle()
            .await?
            .set_progress_handler(PROGRESS_OPS, move || start.elapsed() < limit);
        Ok(())
    }

    async fn kill(_conn: &mut sqlx::SqliteConnection, _id: u64) -> Result<(), sqlx::Error> {
        // Unreachable: `session_id` returns no id, so nothing is ever killed.
        Ok(())
    }
}
/// Creates a lazy `SQLite` pool from a [`DatabaseConfig`].
///
/// Forces `max_connections` to 1 — `SQLite` is a single-writer backend.
/// Opens the file read-only when `config.read_only` is set, backing the
/// statement filter at the database: `SQLite` has no read-only transactions.
/// Otherwise switches `query_only` off on every acquire, undoing
/// [`fetch_read_only`](Connection::fetch_read_only).
fn create_lazy_pool(config: &DatabaseConfig) -> SqlitePool {
    let conn_ops = SqliteConnectOptions::new()
        .filename(config.name.as_deref().unwrap_or_default())
        .read_only(config.read_only);
    let mut pool_opts = sqlx::pool::PoolOptions::new()
        .max_connections(1)
        .min_connections(DatabaseConfig::DEFAULT_MIN_CONNECTIONS)
        .idle_timeout(Duration::from_secs(DatabaseConfig::DEFAULT_IDLE_TIMEOUT_SECS))
        .max_lifetime(Duration::from_secs(DatabaseConfig::DEFAULT_MAX_LIFETIME_SECS));

    if !config.read_only {
        pool_opts = pool_opts.before_acquire(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA query_only = OFF").execute(&mut *conn).await?;
                Ok(true)
            })
        });
    }

    if let Some(timeout) = config.connection_timeout {
        pool_opts = pool_opts.acquire_timeout(Duration::from_secs(timeout));
    }

    pool_opts.connect_lazy_with(conn_ops)
}

/// Quotes `value` as a `SQLite` identifier (ANSI double-quote style).
#[must_use]
pub(crate) fn quote_ident(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbmcp_config::DatabaseBackend;

    /// Counts rows of an endless recursive CTE; only a timeout stops it.
    const ENDLESS: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT COUNT(*) FROM c";

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        }
    }

    #[tokio::test]
    async fn create_lazy_pool_returns_idle_pool() {
        let pool = create_lazy_pool(&base_config());
        assert_eq!(pool.size(), 0, "pool should be lazy (no connections yet)");
    }

    #[tokio::test]
    async fn create_lazy_pool_without_name() {
        let pool = create_lazy_pool(&DatabaseConfig {
            name: None,
            ..base_config()
        });
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn new_creates_lazy_pool() {
        let connection = SqliteConnection::new(&base_config(), None);
        assert_eq!(connection.pool.size(), 0, "pool should be lazy");
    }

    #[tokio::test]
    async fn pool_returns_single_pool() {
        let connection = SqliteConnection::new(&base_config(), None);
        connection.pool(None).await.expect("None target should succeed");
        connection
            .pool(Some("anything"))
            .await
            .expect("any target should return the same single pool");
    }

    #[tokio::test]
    async fn ping_succeeds_on_memory_db() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                ..base_config()
            },
            None,
        );
        connection.ping().await.expect("in-memory database answers");
    }

    #[tokio::test]
    async fn ping_succeeds_while_connection_busy() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                ..base_config()
            },
            None,
        );
        let _busy = connection.pool.acquire().await.expect("acquire the single connection");
        tokio::time::timeout(Duration::from_millis(500), connection.ping())
            .await
            .expect("ping must not wait for the busy connection")
            .expect("busy connection counts as ready");
    }

    #[tokio::test]
    async fn ping_fails_on_missing_file() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some("/nonexistent-dbmcp-dir/missing.db".into()),
                ..base_config()
            },
            None,
        );
        assert!(connection.ping().await.is_err(), "missing file must not ping");
    }

    #[tokio::test]
    async fn read_only_connection_rejects_writes() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                read_only: true,
                ..base_config()
            },
            None,
        );
        let error = connection
            .execute("CREATE TABLE t (id INTEGER)", None)
            .await
            .expect_err("read-only connection must reject writes");
        assert!(
            matches!(error, SqlError::ReadOnlyViolation),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn read_json_rejects_writes_in_read_write_mode() {
        let connection = memory_with_table().await;
        let error = connection
            .read_json("INSERT INTO t (a) VALUES (1) RETURNING id", None, 0)
            .await
            .expect_err("read_json must reject writes");
        assert!(error.to_string().contains("readonly"), "unexpected error: {error}");
        connection
            .execute("INSERT INTO t (a) VALUES (2)", None)
            .await
            .expect("query_only is switched off after the read");
    }

    #[tokio::test]
    async fn read_json_switches_query_only_off_after_timeout() {
        let connection = memory_with_table().await;
        let error = connection
            .read_json(ENDLESS, None, 1)
            .await
            .expect_err("endless read must time out");
        assert!(
            matches!(error, SqlError::QueryTimeout { .. }),
            "unexpected error: {error}"
        );
        connection
            .execute("INSERT INTO t (a) VALUES (1)", None)
            .await
            .expect("query_only is switched off after the timed-out read");
    }

    #[tokio::test]
    async fn dropped_read_leaves_no_query_only_behind() {
        let connection = memory_with_table().await;
        // Drop the read mid-statement.
        let _ = tokio::time::timeout(Duration::from_millis(200), connection.read_json(ENDLESS, None, 1)).await;
        tokio::time::timeout(
            Duration::from_secs(10),
            connection.execute("INSERT INTO t (a) VALUES (1)", None),
        )
        .await
        .expect("connection must come back")
        .expect("before_acquire switches query_only off");
    }

    #[tokio::test]
    async fn read_json_reads_in_read_only_mode() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                read_only: true,
                ..base_config()
            },
            None,
        );
        let rows = connection
            .read_json("SELECT 1 AS v", None, 0)
            .await
            .expect("reads still work");
        assert_eq!(rows, vec![serde_json::json!({"v": 1})]);
    }

    #[tokio::test]
    async fn read_json_rejects_writes_in_read_only_mode() {
        // Without `query_only`, only the read-only file stops this write.
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                read_only: true,
                ..base_config()
            },
            None,
        );
        let error = connection
            .read_json("CREATE TABLE t (id INTEGER)", None, 0)
            .await
            .expect_err("read-only file must reject writes");
        assert!(error.to_string().contains("readonly"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn read_write_connection_allows_writes() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                read_only: false,
                ..base_config()
            },
            None,
        );
        connection
            .execute("CREATE TABLE t (id INTEGER)", None)
            .await
            .expect("read-write connection accepts writes");
    }

    /// Counts an endless recursive CTE, asserting `SQLite` stops it at the 1 s timeout.
    async fn assert_endless_times_out(connection: &SqliteConnection) {
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            connection.read_json(ENDLESS, None, connection.query_timeout()),
        )
        .await
        .expect("statement must stop before the test deadline");
        match result.expect_err("endless statement must time out") {
            // At least 1 s: no stale deadline. Well under the 6 s backstop: SQLite stopped it.
            SqlError::QueryTimeout { elapsed_secs, .. } => {
                assert!((0.9..3.0).contains(&elapsed_secs), "elapsed {elapsed_secs}");
            }
            other => panic!("expected QueryTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn timed_out_statement_stops_and_frees_connection() {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                query_timeout: 1,
                ..base_config()
            },
            None,
        );
        assert_endless_times_out(&connection).await;
        // Each statement gets a fresh deadline.
        assert_endless_times_out(&connection).await;
        let rows = tokio::time::timeout(
            Duration::from_secs(1),
            connection.read_json("SELECT 1 AS v", None, connection.query_timeout()),
        )
        .await
        .expect("connection must be free right after the timeout")
        .expect("fast query succeeds");
        assert_eq!(rows, vec![serde_json::json!({"v": 1})]);
    }

    #[tokio::test]
    async fn shutdown_is_noop_and_idempotent_on_lazy_pool() {
        let connection = SqliteConnection::new(&base_config(), None);
        connection.shutdown().await;
        connection.shutdown().await;
    }

    /// Opens a writable in-memory database holding table `t`.
    ///
    /// The pool's single connection keeps the change count and rowid
    /// between statements, as in production.
    async fn memory_with_table() -> SqliteConnection {
        let connection = SqliteConnection::new(
            &DatabaseConfig {
                name: Some(":memory:".into()),
                read_only: false,
                ..base_config()
            },
            None,
        );
        connection
            .execute("CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER)", None)
            .await
            .expect("create table");
        connection
    }

    async fn counts(connection: &SqliteConnection, sql: &str) -> (u64, Option<u64>) {
        let (_, rows_affected, last_insert_id) = connection.execute_json(sql, None).await.expect(sql);
        (rows_affected, last_insert_id)
    }

    #[tokio::test]
    async fn execute_json_reports_insert_changes_and_rowid() {
        let connection = memory_with_table().await;
        assert_eq!(
            counts(&connection, "INSERT INTO t (a) VALUES (1), (2)").await,
            (2, Some(2))
        );
    }

    #[tokio::test]
    async fn execute_json_counts_replace_as_insert() {
        let connection = memory_with_table().await;
        counts(&connection, "INSERT INTO t (id, a) VALUES (3, 1)").await;
        assert_eq!(
            counts(&connection, "REPLACE INTO t (id, a) VALUES (3, 9)").await,
            (1, Some(3))
        );
    }

    #[tokio::test]
    async fn execute_json_reports_update_and_delete_without_stale_rowid() {
        let connection = memory_with_table().await;
        counts(&connection, "INSERT INTO t (a) VALUES (1), (2)").await;
        assert_eq!(counts(&connection, "UPDATE t SET a = 5").await, (2, None));
        assert_eq!(counts(&connection, "DELETE FROM t").await, (2, None));
    }

    #[tokio::test]
    async fn execute_json_reports_no_rowid_for_noop_upsert() {
        let connection = memory_with_table().await;
        counts(&connection, "INSERT INTO t (id, a) VALUES (1, 1)").await;
        assert_eq!(
            counts(
                &connection,
                "INSERT INTO t (id, a) VALUES (1, 1) ON CONFLICT DO NOTHING"
            )
            .await,
            (0, None)
        );
    }

    #[tokio::test]
    async fn execute_json_ignores_stale_counters_after_schema_change() {
        let connection = memory_with_table().await;
        counts(&connection, "INSERT INTO t (a) VALUES (1)").await;
        assert_eq!(counts(&connection, "CREATE TABLE u (b INTEGER)").await, (0, None));
    }

    #[tokio::test]
    async fn execute_json_pairs_statements_with_results() {
        let connection = memory_with_table().await;
        let sql = "INSERT INTO t (a) VALUES (1); CREATE TABLE u (b INTEGER); UPDATE t SET a = 3";
        assert_eq!(counts(&connection, sql).await, (2, Some(1)));
    }

    #[tokio::test]
    async fn execute_json_reports_zero_for_unparsable_sql() {
        let connection = memory_with_table().await;
        counts(&connection, "CREATE INDEX idx ON t (a)").await;
        counts(&connection, "INSERT INTO t (a) VALUES (1)").await;
        // Valid SQLite that sqlparser rejects.
        assert_eq!(
            counts(&connection, "DELETE FROM t INDEXED BY idx WHERE a = 1").await,
            (0, None)
        );
    }

    #[tokio::test]
    async fn execute_json_returns_returning_rows() {
        let connection = memory_with_table().await;
        let (rows, rows_affected, last_insert_id) = connection
            .execute_json("INSERT INTO t (a) VALUES (7) RETURNING a", None)
            .await
            .expect("insert returning");
        assert_eq!(rows, vec![serde_json::json!({"a": 7})]);
        assert_eq!((rows_affected, last_insert_id), (1, Some(1)));
    }
}
