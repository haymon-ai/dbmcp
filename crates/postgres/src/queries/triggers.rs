//! Trigger listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::PostgresConnection;

/// Brief-mode SQL: `pg_trigger` join with optional `ILIKE` filter on trigger name.
///
/// `'public'::regnamespace` casts the schema name to a `pg_namespace.oid`
/// at plan time, replacing what would otherwise be a `pg_namespace` join.
/// Trigger names are not globally unique on Postgres — the same trigger
/// name can appear on multiple relations — so `c.relname` is the secondary
/// sort key, matching the detailed-mode `ORDER BY` and keeping `OFFSET`
/// pagination stable across duplicate names.
const NAMES: &str = r"
    SELECT t.tgname
    FROM pg_trigger t
    JOIN pg_class c ON t.tgrelid = c.oid
    WHERE c.relnamespace = 'public'::regnamespace
      AND NOT t.tgisinternal
      AND ($1::text IS NULL OR t.tgname ILIKE '%' || $1 || '%')
    ORDER BY t.tgname, c.relname
    LIMIT $2 OFFSET $3";

/// Lists one `page` of trigger names, filtered by `pattern` when given.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn names(
    connection: &PostgresConnection,
    page: Page,
    pattern: Option<&str>,
    database: Option<&str>,
) -> Result<Vec<String>, SqlError> {
    connection
        .fetch_scalar(
            sqlx::query(NAMES).bind(pattern).bind(page.limit()).bind(page.offset()),
            database,
        )
        .await
}

/// Detailed-mode SQL: per-trigger `json_build_object` projection.
///
/// `'public'::regnamespace` casts the schema name to a `pg_namespace.oid`
/// at plan time, eliminating the otherwise-needed `pg_namespace` join.
/// `schema` is hard-coded `'public'` in the output since the WHERE filter
/// already pins it to that one namespace. Postgres defers SELECT-list
/// evaluation past `LIMIT`, so `pg_get_triggerdef` and the events array
/// only run for the page's rows.
///
/// `tgtype` bitmask values are stable since 8.4 and defined in
/// `src/include/catalog/pg_trigger.h`:
/// `TRIGGER_TYPE_ROW`=1, `TRIGGER_TYPE_BEFORE`=2, `TRIGGER_TYPE_INSERT`=4,
/// `TRIGGER_TYPE_DELETE`=8, `TRIGGER_TYPE_UPDATE`=16,
/// `TRIGGER_TYPE_TRUNCATE`=32, `TRIGGER_TYPE_INSTEAD`=64.
const DETAILS: &str = r"
    SELECT
        t.tgname AS name,
        json_build_object(
            'schema',          'public',
            'table',           c.relname,
            'status',          CASE t.tgenabled
                                   WHEN 'O' THEN 'ENABLED'
                                   WHEN 'D' THEN 'DISABLED'
                                   WHEN 'R' THEN 'REPLICA'
                                   WHEN 'A' THEN 'ALWAYS'
                               END,
            'timing',          CASE
                                   WHEN (t.tgtype & 2)  = 2  THEN 'BEFORE'
                                   WHEN (t.tgtype & 64) = 64 THEN 'INSTEAD OF'
                                   ELSE 'AFTER'
                               END,
            'events',          to_json(array_remove(ARRAY[
                                   CASE WHEN (t.tgtype & 4)  = 4  THEN 'INSERT'   END,
                                   CASE WHEN (t.tgtype & 16) = 16 THEN 'UPDATE'   END,
                                   CASE WHEN (t.tgtype & 8)  = 8  THEN 'DELETE'   END,
                                   CASE WHEN (t.tgtype & 32) = 32 THEN 'TRUNCATE' END
                               ], NULL)),
            'activationLevel', CASE WHEN (t.tgtype & 1) = 1 THEN 'ROW' ELSE 'STATEMENT' END,
            'functionName',    p.proname,
            'definition',      pg_get_triggerdef(t.oid)
        ) AS entry
    FROM pg_trigger t
    JOIN pg_class c ON t.tgrelid = c.oid
    LEFT JOIN pg_proc p ON p.oid = t.tgfoid
    WHERE c.relnamespace = 'public'::regnamespace
      AND NOT t.tgisinternal
      AND ($1::text IS NULL OR t.tgname ILIKE '%' || $1 || '%')
    ORDER BY t.tgname, c.relname
    LIMIT $2 OFFSET $3";

/// Lists one `page` of trigger entries with details, filtered by `pattern` when given.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed query.
pub(crate) async fn details(
    connection: &PostgresConnection,
    page: Page,
    pattern: Option<&str>,
    database: Option<&str>,
) -> Result<Vec<(String, Value)>, SqlError> {
    connection
        .fetch(
            sqlx::query(DETAILS)
                .bind(pattern)
                .bind(page.limit())
                .bind(page.offset()),
            database,
        )
        .await
}
