//! Catalog snapshot: every catalog tool's rows, read once per database and cached.
//!
//! [`MysqlConnection::snapshot`] fills a [`Snapshot`] for one database on
//! first use and keeps it until the cache TTL expires;
//! [`MysqlConnection::databases`] does the same for the database list.
//! The catalog tools page and filter them in memory, with output identical
//! to their live SQL.

use std::sync::Arc;

use dbmcp_sql::Connection as _;
use rmcp::model::ErrorData;
use serde_json::Value;

use crate::connection::MysqlConnection;
use crate::tools::{list_databases, list_functions, list_procedures, list_tables, list_triggers, list_views};

/// `LIMIT` large enough for every row: `MySQL` has no unbounded `LIMIT` form.
const ALL_ROWS: i64 = i64::MAX;

/// Unpaged, unfiltered detailed rows of every `MySQL`/`MariaDB` catalog tool.
///
/// Each field holds that tool's `DETAILED_SQL` rows in the query's
/// `ORDER BY` order.
#[derive(Debug)]
pub(crate) struct Snapshot {
    /// `listTables` rows.
    pub(crate) tables: Vec<(String, Value)>,
    /// `listViews` rows.
    pub(crate) views: Vec<(String, Value)>,
    /// `listFunctions` rows.
    pub(crate) functions: Vec<(String, Value)>,
    /// `listProcedures` rows.
    pub(crate) procedures: Vec<(String, Value)>,
    /// `listTriggers` rows.
    pub(crate) triggers: Vec<(String, Value)>,
}

impl MysqlConnection {
    /// Returns the cached snapshot of `database`; `None` when caching is off.
    ///
    /// Fills on first use. `database` is the resolved name (trimmed argument, else the default,
    /// possibly `""`). Every query runs on the default pool with the name
    /// bound, as the live tools do. Concurrent first callers wait on one
    /// fill. A failed fill caches nothing and yields the [`ErrorData`] the
    /// live catalog query would return, so the next call retries.
    pub(crate) async fn snapshot(&self, database: &str) -> Option<Result<Arc<Snapshot>, ErrorData>> {
        let cache = self.cache.as_ref()?;
        let snapshot = cache
            .try_get_with_by_ref(database, async {
                let no_search: Option<&str> = None;
                let query = |sql: &'static str| {
                    sqlx::query(sql)
                        .bind(database)
                        .bind(no_search)
                        .bind(no_search)
                        .bind(ALL_ROWS)
                        .bind(0_i64)
                };
                let rows = async |sql: &'static str| self.fetch(query(sql), None).await;
                // `listTables` binds the database a second time, for its partitions CTE.
                let tables = self
                    .fetch(query(list_tables::DETAILED_SQL).bind(database), None)
                    .await?;
                Ok::<_, ErrorData>(Arc::new(Snapshot {
                    tables,
                    views: rows(list_views::DETAILED_SQL).await?,
                    functions: rows(list_functions::DETAILED_SQL).await?,
                    procedures: rows(list_procedures::DETAILED_SQL).await?,
                    triggers: rows(list_triggers::DETAILED_SQL).await?,
                }))
            })
            .await
            .map_err(Arc::unwrap_or_clone);
        Some(snapshot)
    }

    /// Returns the cached database names; `None` when caching is off.
    ///
    /// Fills on first use. Reads on the default pool, as `listDatabases` does. A failed fill
    /// caches nothing and yields the live query's [`ErrorData`].
    pub(crate) async fn databases(&self) -> Option<Result<Arc<Vec<String>>, ErrorData>> {
        let cache = self.databases.as_ref()?;
        let databases = cache
            .try_get_with((), async {
                let query = sqlx::query(list_databases::SQL).bind(ALL_ROWS).bind(0_i64);
                Ok::<_, ErrorData>(Arc::new(self.fetch_scalar(query, None).await?))
            })
            .await
            .map_err(Arc::unwrap_or_clone);
        Some(databases)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dbmcp_config::{CacheConfig, DatabaseBackend, DatabaseConfig};

    use super::*;

    fn connection(cache: Option<CacheConfig>) -> MysqlConnection {
        MysqlConnection::new(
            &DatabaseConfig {
                backend: DatabaseBackend::Mysql,
                host: "db.example.com".into(),
                user: "admin".into(),
                ..DatabaseConfig::default()
            },
            cache,
        )
    }

    #[tokio::test]
    async fn no_caches_without_cache_config() {
        let connection = connection(None);
        assert!(connection.snapshot("").await.is_none());
        assert!(connection.databases().await.is_none());
    }

    #[test]
    fn caches_built_with_cache_config() {
        let connection = connection(Some(CacheConfig {
            ttl: Duration::from_mins(1),
        }));
        assert!(connection.cache.is_some());
        assert!(connection.databases.is_some());
    }
}
