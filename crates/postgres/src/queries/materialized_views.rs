//! Materialized view listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::PostgresConnection;

/// Brief-mode SQL: `pg_matviews` scan with `ILIKE` filter on matview name.
///
/// `pg_matviews` already filters to `pg_class.relkind = 'm'`, and
/// `schemaname = 'public'` keeps system-schema matviews out. The
/// `($1::text IS NULL OR ...)` trinary lets one statement cover both filtered
/// and unfiltered cases. Matview names are unique per schema (Postgres enforces
/// this via `pg_class`'s unique index on `(relname, relnamespace)`), so
/// `matviewname` alone is a stable sort key — no tiebreaker needed.
const NAMES: &str = r"
    SELECT matviewname
    FROM pg_matviews
    WHERE schemaname = 'public'
      AND ($1::text IS NULL OR matviewname ILIKE '%' || $1 || '%')
    ORDER BY matviewname
    LIMIT $2 OFFSET $3";

/// Lists one `page` of materialized view names, filtered by `pattern` when given.
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

/// Detailed-mode SQL: per-matview `json_build_object` projection.
///
/// `pg_matviews` already filters to `relkind = 'm'`. The `pg_namespace` +
/// `pg_class` joins anchor the relation OID needed by `obj_description`.
/// Postgres defers SELECT-list evaluation past `LIMIT`, so `obj_description`
/// only runs for the page's rows — never the full schema.
/// `pg_matviews.matviewowner` is already a role name, so no `pg_roles` join is
/// needed. `populated` and `indexed` are projected directly from
/// `pg_matviews.ispopulated` / `pg_matviews.hasindexes`. Matview names are
/// unique per schema, so `matviewname` alone is a stable sort key.
const DETAILS: &str = r"
    SELECT
        mv.matviewname AS name,
        json_build_object(
            'schema',      mv.schemaname,
            'owner',       mv.matviewowner,
            'description', pg_catalog.obj_description(c.oid, 'pg_class'),
            'definition',  mv.definition,
            'populated',   mv.ispopulated,
            'indexed',     mv.hasindexes
        ) AS entry
    FROM pg_matviews mv
    JOIN pg_namespace n ON n.nspname = mv.schemaname
    JOIN pg_class     c ON c.relname = mv.matviewname AND c.relnamespace = n.oid
    WHERE mv.schemaname = 'public'
      AND ($1::text IS NULL OR mv.matviewname ILIKE '%' || $1 || '%')
    ORDER BY mv.matviewname
    LIMIT $2 OFFSET $3";

/// Lists one `page` of materialized view entries with details, filtered by `pattern` when given.
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
