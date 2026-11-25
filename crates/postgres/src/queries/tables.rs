//! Table listing statements and `DROP TABLE`.

use dbmcp_server::listing::Page;
use dbmcp_sql::{Connection as _, SqlError};
use serde_json::Value;

use super::ident;
use crate::connection::PostgresConnection;

/// Brief-mode SQL: single `pg_tables` scan with optional `ILIKE` filter.
const NAMES: &str = r"
    SELECT tablename
    FROM pg_tables
    WHERE schemaname = 'public'
      AND ($1::text IS NULL OR tablename ILIKE '%' || $1 || '%')
    ORDER BY tablename
    LIMIT $2 OFFSET $3";

/// Lists one `page` of table names, filtered by `pattern` when given.
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

/// Detailed-mode SQL: single CTE returning one `json_build_object` per table.
///
/// `LIMIT`/`OFFSET` are pushed into `table_info` so every downstream CTE
/// scans at most `page_size + 1` tables, never the full schema.
const DETAILS: &str = r"
    WITH table_info AS (
        SELECT
            t.oid AS table_oid,
            ns.nspname AS schema_name,
            t.relname AS table_name,
            pg_get_userbyid(t.relowner) AS table_owner,
            obj_description(t.oid, 'pg_class') AS table_comment,
            t.relkind AS object_kind
        FROM pg_class t
        JOIN pg_namespace ns ON ns.oid = t.relnamespace
        WHERE t.relkind IN ('r', 'p')
          AND ns.nspname = 'public'
          AND ($1::text IS NULL OR t.relname ILIKE '%' || $1 || '%')
        ORDER BY t.relname
        LIMIT $2 OFFSET $3
    ),
    columns_info AS (
        SELECT
            att.attrelid AS table_oid,
            att.attname AS column_name,
            format_type(att.atttypid, att.atttypmod) AS data_type,
            att.attnum AS ordinal_position,
            NOT att.attnotnull AS nullable,
            pg_get_expr(ad.adbin, ad.adrelid) AS column_default,
            col_description(att.attrelid, att.attnum) AS column_comment
        FROM pg_attribute att
        LEFT JOIN pg_attrdef ad ON att.attrelid = ad.adrelid AND att.attnum = ad.adnum
        JOIN table_info ti ON att.attrelid = ti.table_oid
        WHERE att.attnum > 0 AND NOT att.attisdropped
    ),
    constraints_info AS (
        SELECT
            con.conrelid AS table_oid,
            con.conname AS constraint_name,
            pg_get_constraintdef(con.oid) AS constraint_definition,
            CASE con.contype
                WHEN 'p' THEN 'PRIMARY KEY'
                WHEN 'f' THEN 'FOREIGN KEY'
                WHEN 'u' THEN 'UNIQUE'
                WHEN 'c' THEN 'CHECK'
                ELSE con.contype::text
            END AS constraint_type,
            COALESCE((
                SELECT array_agg(att.attname ORDER BY u.attposition)
                FROM unnest(con.conkey) WITH ORDINALITY AS u(attnum, attposition)
                JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = u.attnum
            ), ARRAY[]::name[]) AS constraint_columns,
            CASE WHEN con.confrelid <> 0
                 THEN (con.confrelid::regclass)::text
                 ELSE NULL END AS referenced_table,
            (
                SELECT array_agg(att.attname ORDER BY u.attposition)
                FROM unnest(con.confkey) WITH ORDINALITY AS u(attnum, attposition)
                JOIN pg_attribute att ON att.attrelid = con.confrelid AND att.attnum = u.attnum
                WHERE con.contype = 'f'
            ) AS referenced_columns
        FROM pg_constraint con
        JOIN table_info ti ON con.conrelid = ti.table_oid
        WHERE con.contype IN ('p', 'f', 'u', 'c')
    ),
    indexes_info AS (
        SELECT
            idx.indrelid AS table_oid,
            ic.relname AS index_name,
            pg_get_indexdef(idx.indexrelid) AS index_definition,
            idx.indisunique AS is_unique,
            idx.indisprimary AS is_primary,
            am.amname AS index_method,
            (
                SELECT array_agg(att.attname ORDER BY u.ord)
                FROM unnest(idx.indkey::int[]) WITH ORDINALITY AS u(colidx, ord)
                LEFT JOIN pg_attribute att ON att.attrelid = idx.indrelid AND att.attnum = u.colidx
                WHERE u.colidx <> 0
            ) AS index_columns
        FROM pg_index idx
        JOIN pg_class ic ON ic.oid = idx.indexrelid
        JOIN pg_am am ON am.oid = ic.relam
        JOIN table_info ti ON idx.indrelid = ti.table_oid
    ),
    triggers_info AS (
        SELECT
            tg.tgrelid AS table_oid,
            tg.tgname AS trigger_name,
            pg_get_triggerdef(tg.oid) AS trigger_definition,
            tg.tgenabled::text AS trigger_enabled
        FROM pg_trigger tg
        JOIN table_info ti ON tg.tgrelid = ti.table_oid
        WHERE NOT tg.tgisinternal
    )
    SELECT ti.table_name AS name, json_build_object(
        'schema',  ti.schema_name,
        'kind',    CASE ti.object_kind
                       WHEN 'r' THEN 'TABLE'
                       WHEN 'p' THEN 'PARTITIONED_TABLE'
                       ELSE ti.object_kind::text
                   END,
        'owner',   ti.table_owner,
        'comment', ti.table_comment,
        'columns', COALESCE((
            SELECT json_agg(json_build_object(
                'name',            ci.column_name,
                'dataType',        ci.data_type,
                'ordinalPosition', ci.ordinal_position,
                'nullable',        ci.nullable,
                'default',         ci.column_default,
                'comment',         ci.column_comment
            ) ORDER BY ci.ordinal_position)
            FROM columns_info ci WHERE ci.table_oid = ti.table_oid
        ), '[]'::json),
        'constraints', COALESCE((
            SELECT json_agg(json_build_object(
                'name',              cons.constraint_name,
                'type',              cons.constraint_type,
                'columns',           cons.constraint_columns,
                'definition',        cons.constraint_definition,
                'referencedTable',   cons.referenced_table,
                'referencedColumns', cons.referenced_columns
            ) ORDER BY cons.constraint_name)
            FROM constraints_info cons WHERE cons.table_oid = ti.table_oid
        ), '[]'::json),
        'indexes', COALESCE((
            SELECT json_agg(json_build_object(
                'name',       ii.index_name,
                'columns',    ii.index_columns,
                'unique',     ii.is_unique,
                'primary',    ii.is_primary,
                'method',     ii.index_method,
                'definition', ii.index_definition
            ) ORDER BY ii.index_name)
            FROM indexes_info ii WHERE ii.table_oid = ti.table_oid
        ), '[]'::json),
        'triggers', COALESCE((
            SELECT json_agg(json_build_object(
                'name',       tri.trigger_name,
                'definition', tri.trigger_definition,
                'enabled',    tri.trigger_enabled
            ) ORDER BY tri.trigger_name)
            FROM triggers_info tri WHERE tri.table_oid = ti.table_oid
        ), '[]'::json)
    ) AS entry
    FROM table_info ti
    ORDER BY ti.schema_name, ti.table_name";

/// Lists one `page` of table entries with details, filtered by `pattern` when given.
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

/// Statement prefix of `drop`; the quoted table name follows.
const DROP: &str = "DROP TABLE";

/// Suffix of `drop` that also removes dependent foreign-key constraints.
const CASCADE: &str = " CASCADE";

/// Drops `table` in `database`, quoted as an identifier; `cascade` appends `CASCADE`.
///
/// # Errors
///
/// Returns the [`SqlError`] of a failed statement, or
/// [`SqlError::ReadOnlyViolation`] in read-only mode.
pub(crate) async fn drop(
    connection: &PostgresConnection,
    table: &str,
    database: Option<&str>,
    cascade: bool,
) -> Result<(), SqlError> {
    let mut sql = format!("{DROP} {}", ident::quote(table));
    if cascade {
        sql.push_str(CASCADE);
    }
    connection.execute(sql.as_str(), database).await?;
    Ok(())
}
