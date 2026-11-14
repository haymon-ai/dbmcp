//! Catalog snapshot: every catalog tool's rows, read once and cached.
//!
//! [`SqliteConnection::snapshot`] fills a [`Snapshot`] on first use and
//! keeps it until the cache TTL expires. The catalog tools page and
//! filter it in memory, with output identical to their live SQL.

use std::sync::Arc;

use dbmcp_sql::Connection as _;
use rmcp::model::ErrorData;
use serde_json::Value;

use crate::connection::SqliteConnection;
use crate::tools::{list_tables, list_triggers, list_views};

/// `LIMIT` that makes `SQLite` return every row.
const ALL_ROWS: i64 = -1;

/// Unpaged, unfiltered rows of every `SQLite` catalog tool.
#[derive(Debug)]
pub(crate) struct Snapshot {
    /// `listTables` detailed rows, in the query's `ORDER BY` order.
    pub(crate) tables: Vec<(String, Value)>,
    /// `listTriggers` detailed rows, in the query's `ORDER BY` order.
    pub(crate) triggers: Vec<(String, Value)>,
    /// `listViews` names, in the query's `ORDER BY` order.
    pub(crate) views: Vec<String>,
}

impl SqliteConnection {
    /// Returns the cached snapshot, filling it on first use; `None` when caching is off.
    ///
    /// Concurrent first callers wait on one fill. A failed fill caches
    /// nothing and yields the [`ErrorData`] the live catalog query would
    /// return, so the next call retries.
    pub(crate) async fn snapshot(&self) -> Option<Result<Arc<Snapshot>, ErrorData>> {
        let cache = self.cache.as_ref()?;
        let snapshot = cache
            .try_get_with((), async {
                let no_search: Option<&str> = None;
                let tables = self
                    .fetch(
                        sqlx::query(list_tables::DETAILED_SQL)
                            .bind(no_search)
                            .bind(ALL_ROWS)
                            .bind(0_i64),
                        None,
                    )
                    .await?;
                let triggers = self
                    .fetch(
                        sqlx::query(list_triggers::DETAILED_SQL)
                            .bind(no_search)
                            .bind(ALL_ROWS)
                            .bind(0_i64),
                        None,
                    )
                    .await?;
                let views = self
                    .fetch_scalar(sqlx::query(list_views::SQL).bind(ALL_ROWS).bind(0_i64), None)
                    .await?;
                Ok::<_, ErrorData>(Arc::new(Snapshot {
                    tables,
                    triggers,
                    views,
                }))
            })
            .await
            .map_err(Arc::unwrap_or_clone);
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dbmcp_config::{CacheConfig, DatabaseBackend, DatabaseConfig};

    use super::*;

    fn connection(name: &str, cache: Option<CacheConfig>) -> SqliteConnection {
        SqliteConnection::new(
            &DatabaseConfig {
                backend: DatabaseBackend::Sqlite,
                name: Some(name.into()),
                read_only: false,
                ..DatabaseConfig::default()
            },
            cache,
        )
    }

    fn cached(name: &str) -> SqliteConnection {
        connection(
            name,
            Some(CacheConfig {
                ttl: Duration::from_mins(1),
            }),
        )
    }

    async fn snapshot(connection: &SqliteConnection) -> Result<Arc<Snapshot>, ErrorData> {
        connection.snapshot().await.expect("cache on")
    }

    #[tokio::test]
    async fn snapshot_holds_every_kind_in_order() {
        let connection = cached(":memory:");
        for sql in [
            "CREATE TABLE b (id INTEGER)",
            "CREATE TABLE a (id INTEGER)",
            "CREATE VIEW v2 AS SELECT 1",
            "CREATE VIEW v1 AS SELECT 1",
            "CREATE TRIGGER t1 AFTER INSERT ON a BEGIN SELECT 1; END",
        ] {
            connection.execute(sql, None).await.expect(sql);
        }
        let snapshot = snapshot(&connection).await.expect("fill");

        let tables: Vec<_> = snapshot.tables.iter().map(|(name, _)| name.as_str()).collect();
        let triggers: Vec<_> = snapshot.triggers.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(tables, ["a", "b"]);
        assert_eq!(triggers, ["t1"]);
        assert_eq!(snapshot.views, ["v1", "v2"]);
    }

    #[tokio::test]
    async fn concurrent_first_calls_share_one_fill() {
        let connection = cached(":memory:");
        let (first, second) = tokio::join!(snapshot(&connection), snapshot(&connection));
        assert!(
            Arc::ptr_eq(&first.expect("first"), &second.expect("second")),
            "both callers must get the one filled snapshot"
        );
    }

    #[tokio::test]
    async fn failed_fill_caches_nothing() {
        // The pool keeps the file open, so the directory's drop cleans it up; Windows
        // refuses to remove an open file.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("cache.db");
        let connection = cached(path.to_str().expect("utf-8 path"));

        let error = snapshot(&connection).await.expect_err("missing file must fail");
        assert!(error.message.contains("Database error"), "unexpected error: {error:?}");

        std::fs::File::create(&path).expect("create database file");
        snapshot(&connection).await.expect("next call retries the fill");
    }

    #[tokio::test]
    async fn no_snapshot_without_cache_config() {
        assert!(connection(":memory:", None).snapshot().await.is_none());
    }
}
