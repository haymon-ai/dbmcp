//! MCP tool: `listTables`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;

use crate::types::{ListTablesRequest, ListTablesResponse};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listTables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "listTables";
    const TITLE: &'static str = "List Tables";
    const DESCRIPTION: &'static str = r#"List tables in a database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Exploring a database to find relevant tables (brief mode, default).
- Searching for a table by partial name (pass `search`).
- Inspecting a table's columns, constraints, indexes, and triggers before writing a query (pass `detailed: true`). This supersedes the legacy `getTableSchema` tool.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on table names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by table name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What tables are in mydb?" → listTables(database="mydb")
✓ "Find the orders table" → listTables(search="order")
✓ "What columns does orders have?" → listTables(search="orders", detailed=true)
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of table-name strings, e.g. `["customers", "orders"]`.
Detailed mode: a JSON object keyed by table name; each value carries `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, and `triggers` (the name is the key and is not repeated inside the value). Keys iterate in the same alphabetical order brief mode uses. Example: `{"orders": {"schema": "public", "kind": "TABLE", …}}`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>"#;
}

impl ToolBase for ListTablesTool {
    type Parameter = ListTablesRequest;
    type Output = ListTablesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for ListTablesTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_tables(params).await
    }
}

/// Brief-mode SQL: single `pg_tables` scan with optional `ILIKE` filter.
const BRIEF_SQL: &str = r"
    SELECT tablename
    FROM pg_tables
    WHERE schemaname = 'public'
      AND ($1::text IS NULL OR tablename ILIKE '%' || $1 || '%')
    ORDER BY tablename
    LIMIT $2 OFFSET $3";

/// Detailed-mode SQL: single CTE returning one `json_build_object` per table.
///
/// `LIMIT`/`OFFSET` are pushed into `table_info` so every downstream CTE
/// scans at most `page_size + 1` tables, never the full schema.
const DETAILED_SQL: &str = r"
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
            ))
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
            ))
            FROM indexes_info ii WHERE ii.table_oid = ti.table_oid
        ), '[]'::json),
        'triggers', COALESCE((
            SELECT json_agg(json_build_object(
                'name',       tri.trigger_name,
                'definition', tri.trigger_definition,
                'enabled',    tri.trigger_enabled
            ))
            FROM triggers_info tri WHERE tri.table_oid = ti.table_oid
        ), '[]'::json)
    ) AS entry
    FROM table_info ti
    ORDER BY ti.schema_name, ti.table_name";

impl PostgresHandler {
    /// Lists one page of tables in a database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListTablesRequest,
    ) -> Result<ListTablesResponse, ErrorData> {
        // Identifier validation runs inside the connection pool — passing the
        // trimmed name straight through avoids a redundant round-trip check.
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pattern = search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pager = Pager::new(cursor, self.config.page_size);

        if detailed {
            let rows: Vec<(String, sqlx::types::Json<serde_json::Value>)> = self
                .connection
                .fetch(
                    sqlx::query(DETAILED_SQL)
                        .bind(pattern)
                        .bind(pager.limit())
                        .bind(pager.offset()),
                    database,
                )
                .await?;
            let (rows, next_cursor) = pager.paginate(rows);
            return Ok(ListTablesResponse::detailed(
                rows.into_iter().map(|(name, json)| (name, json.0)).collect(),
                next_cursor,
            ));
        }

        let rows: Vec<String> = self
            .connection
            .fetch_scalar(
                sqlx::query(BRIEF_SQL)
                    .bind(pattern)
                    .bind(pager.limit())
                    .bind(pager.offset()),
                database,
            )
            .await?;
        let (tables, next_cursor) = pager.paginate(rows);
        Ok(ListTablesResponse::brief(tables, next_cursor))
    }
}
