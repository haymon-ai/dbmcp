//! Catalog snapshot: every catalog tool's rows, read once per database and cached.
//!
//! [`PostgresConnection::snapshot`] fills a [`Snapshot`] for one database on
//! first use and keeps it until the cache TTL expires;
//! [`PostgresConnection::databases`] does the same for the database list.
//! The catalog tools page and filter them in memory, with output identical
//! to their live SQL.

use std::sync::Arc;

use dbmcp_server::listing::{self, Cursor, Like};
use dbmcp_server::types::ListEntriesResponse;
use dbmcp_sql::Connection as _;
use rmcp::model::ErrorData;
use serde_json::Value;

use super::{
    list_databases, list_functions, list_materialized_views, list_procedures, list_tables, list_triggers, list_views,
};
use crate::connection::PostgresConnection;

/// `LIMIT NULL`: makes `PostgreSQL` return every row.
const ALL_ROWS: Option<i64> = None;

/// Unpaged, unfiltered detailed rows of every `PostgreSQL` catalog tool.
///
/// Each field holds that tool's `DETAILED_SQL` rows in the query's
/// `ORDER BY` order.
#[derive(Debug)]
pub(crate) struct Snapshot {
    /// `listTables` rows.
    pub(crate) tables: Vec<(String, Value)>,
    /// `listViews` rows.
    pub(crate) views: Vec<(String, Value)>,
    /// `listMaterializedViews` rows.
    pub(crate) materialized_views: Vec<(String, Value)>,
    /// `listFunctions` rows, keyed by signature `name(args)`.
    pub(crate) functions: Vec<(String, Value)>,
    /// `listProcedures` rows, keyed by signature `name(args)`.
    pub(crate) procedures: Vec<(String, Value)>,
    /// `listTriggers` rows.
    pub(crate) triggers: Vec<(String, Value)>,
}

impl PostgresConnection {
    /// Returns the cached snapshot of `database`, filling it on first use; `None` when caching is off.
    ///
    /// `database` is the trimmed tool argument; `None` means the default
    /// database. Concurrent first callers wait on one fill. A failed fill
    /// caches nothing and yields the [`ErrorData`] the live catalog query
    /// would return, so the next call retries.
    pub(crate) async fn snapshot(&self, database: Option<&str>) -> Option<Result<Arc<Snapshot>, ErrorData>> {
        let cache = self.catalog.as_ref()?;
        let database = database.unwrap_or_else(|| self.default_database_name());
        let snapshot = cache
            .try_get_with_by_ref(database, async {
                let rows = async |sql: &'static str| -> Result<Vec<(String, Value)>, ErrorData> {
                    let no_search: Option<&str> = None;
                    let query = sqlx::query(sql).bind(no_search).bind(ALL_ROWS).bind(0_i64);
                    Ok(self.fetch(query, Some(database)).await?)
                };
                Ok::<_, ErrorData>(Arc::new(Snapshot {
                    tables: rows(list_tables::DETAILED_SQL).await?,
                    views: rows(list_views::DETAILED_SQL).await?,
                    materialized_views: rows(list_materialized_views::DETAILED_SQL).await?,
                    functions: rows(list_functions::DETAILED_SQL).await?,
                    procedures: rows(list_procedures::DETAILED_SQL).await?,
                    triggers: rows(list_triggers::DETAILED_SQL).await?,
                }))
            })
            .await
            .map_err(Arc::unwrap_or_clone);
        Some(snapshot)
    }

    /// Returns the cached database names, filling them on first use; `None` when caching is off.
    ///
    /// Reads on the default pool, as `listDatabases` does. A failed fill
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

/// Pages cached function or procedure rows like their live SQL.
///
/// Live SQL filters and lists brief names by `proname`, but keys detailed
/// rows by signature. Each row's entry carries `proname` as `name`.
pub(super) fn list_routines(
    rows: &[(String, Value)],
    cursor: Option<Cursor>,
    page_size: u16,
    search: Option<&str>,
    detailed: bool,
) -> ListEntriesResponse {
    let (page, next_cursor) = listing::page_matching(rows, proname, cursor, page_size, search, Like::POSTGRES);

    if detailed {
        let entries = page.into_iter().map(|(key, entry)| (key.clone(), entry.clone()));
        return ListEntriesResponse::detailed(entries.collect(), next_cursor);
    }

    let names = page.into_iter().map(|row| proname(row).to_owned()).collect();
    ListEntriesResponse::brief(names, next_cursor)
}

/// Returns a function or procedure row's `proname`, stored in its entry as `name`.
fn proname((_, entry): &(String, Value)) -> &str {
    entry["name"].as_str().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dbmcp_config::{CacheConfig, DatabaseBackend, DatabaseConfig};
    use serde_json::json;

    use super::*;

    fn connection(cache: Option<CacheConfig>) -> PostgresConnection {
        PostgresConnection::new(
            &DatabaseConfig {
                backend: DatabaseBackend::Postgres,
                host: "pg.example.com".into(),
                user: "pgadmin".into(),
                ..DatabaseConfig::default()
            },
            cache,
        )
    }

    #[tokio::test]
    async fn no_caches_without_cache_config() {
        let connection = connection(None);
        assert!(connection.snapshot(None).await.is_none());
        assert!(connection.databases().await.is_none());
    }

    #[test]
    fn caches_built_with_cache_config() {
        let connection = connection(Some(CacheConfig {
            ttl: Duration::from_mins(1),
        }));
        assert!(connection.catalog.is_some());
        assert!(connection.databases.is_some());
    }

    fn routine(name: &str, args: &str) -> (String, Value) {
        (format!("{name}({args})"), json!({ "name": name, "arguments": args }))
    }

    fn routines() -> Vec<(String, Value)> {
        vec![
            routine("add", "a integer"),
            routine("add", "a text"),
            routine("tag", "integer"),
            routine("tally", ""),
        ]
    }

    #[test]
    fn list_routines_brief_lists_proname_per_overload() {
        let response = list_routines(&routines(), None, 10, None, false);
        assert_eq!(
            response.entries.as_brief().expect("brief"),
            ["add", "add", "tag", "tally"]
        );
    }

    #[test]
    fn list_routines_detailed_keys_by_signature() {
        let response = list_routines(&routines(), None, 10, None, true);
        let value = serde_json::to_value(&response).expect("serialize");
        let keys: Vec<_> = value["entries"]
            .as_object()
            .expect("detailed")
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["add(a integer)", "add(a text)", "tag(integer)", "tally()"]);
    }

    #[test]
    fn list_routines_searches_proname_not_signature() {
        let response = list_routines(&routines(), None, 10, Some("integer"), false);
        assert!(response.entries.as_brief().expect("brief").is_empty());
        let response = list_routines(&routines(), None, 10, Some("TA"), false);
        assert_eq!(response.entries.as_brief().expect("brief"), ["tag", "tally"]);
    }

    #[test]
    fn list_routines_pages_with_cursor() {
        let first = list_routines(&routines(), None, 3, None, false);
        assert_eq!(first.entries.as_brief().expect("brief"), ["add", "add", "tag"]);
        let second = list_routines(&routines(), first.next_cursor, 3, None, false);
        assert_eq!(second.entries.as_brief().expect("brief"), ["tally"]);
        assert!(second.next_cursor.is_none());
    }
}
