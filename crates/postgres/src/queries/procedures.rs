//! Procedure listing statements.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use crate::connection::PostgresConnection;

/// Brief-mode SQL: `pg_proc` scan with `ILIKE` filter on procedure name.
///
/// `n.nspname = 'public'` plus `p.prokind = 'p'` keep functions / aggregates /
/// window functions out of the result. The `($1::text IS NULL OR ...)` trinary
/// lets one statement cover both filtered and unfiltered cases. `(p.proname,
/// p.oid)` is the sort key — `oid` is the unique tiebreaker across overloaded
/// names so `OFFSET` pagination is deterministic.
const NAMES: &str = r"
    SELECT p.proname
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND p.prokind = 'p'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

/// Lists one `page` of procedure names, filtered by `pattern` when given.
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

/// Detailed-mode SQL: per-procedure `json_build_object` projection.
///
/// `n.nspname = 'public'` and `p.prokind = 'p'` filter to user-defined
/// procedures in the `public` schema. Three small lookup joins (`pg_namespace`,
/// `pg_language`, `pg_roles`) supply the language and owner names. Postgres
/// defers SELECT-list evaluation past `LIMIT`, so the expensive `pg_get_*`
/// projections (`pg_get_functiondef`, `pg_get_function_arguments`) and
/// `obj_description` only run for the page's rows — never the full schema.
///
/// A `CROSS JOIN LATERAL` materialises `pg_get_function_arguments(p.oid)` into
/// `args.text` so it is computed once per row and reused both in the keyed
/// signature `name(args)` and in the JSON `arguments` field — no double-call.
///
/// `prosecdef` is a boolean. Procedure-only fields (`returnType`, `volatility`,
/// `strict`, `parallelSafety`) are deliberately absent — see the tool description above.
const DETAILS: &str = r"
    SELECT
        p.proname || '(' || args.text || ')' AS name,
        json_build_object(
            'schema',      'public',
            'name',        p.proname,
            'language',    l.lanname,
            'arguments',   args.text,
            'security',    CASE WHEN p.prosecdef THEN 'DEFINER' ELSE 'INVOKER' END,
            'owner',       r.rolname,
            'description', pg_catalog.obj_description(p.oid, 'pg_proc'),
            'definition',  pg_get_functiondef(p.oid)
        ) AS entry
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    JOIN pg_language  l ON l.oid = p.prolang
    JOIN pg_roles     r ON r.oid = p.proowner
    CROSS JOIN LATERAL (SELECT pg_get_function_arguments(p.oid) AS text) args
    WHERE n.nspname = 'public'
      AND p.prokind = 'p'
      AND ($1::text IS NULL OR p.proname ILIKE '%' || $1 || '%')
    ORDER BY p.proname, p.oid
    LIMIT $2 OFFSET $3";

/// Lists one `page` of procedure entries with details, filtered by `pattern` when given.
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
