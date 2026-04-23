//! MCP tool: `listTables`.

use std::borrow::Cow;

use dbmcp_server::pagination::{Cursor, Pager};
use dbmcp_sql::Connection;
use dbmcp_sql::sanitize::validate_ident;

use crate::types::{ListTablesRequest, ListTablesResponse, TableEntries};
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
- `search` — Case-insensitive substring filter on table names. `%`, `_`, `\` are literal.
- `detailed` — When `true`, returns full metadata objects instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What tables are in mydb?" → listTables(database="mydb")
✓ "Find the orders table" → listTables(search="order")
✓ "What columns does orders have?" → listTables(search="orders", detailed=true)
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of table-name strings.
Detailed mode: a sorted JSON array of objects with `schema`, `name`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, and `triggers`.
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
      AND ($1::text IS NULL OR tablename ILIKE $1 ESCAPE '\')
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
          AND ($1::text IS NULL OR t.relname ILIKE $1 ESCAPE '\')
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
    SELECT json_build_object(
        'schema',  ti.schema_name,
        'name',    ti.table_name,
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

/// Escapes LIKE metacharacters (`\`, `%`, `_`) in a substring search term.
///
/// Returned value is safe to concatenate into a `%...%` pattern bound with
/// `ILIKE $1 ESCAPE '\'`. Does not quote — that is sqlx's job.
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Builds the `Option<String>` pattern bound as `$1` for brief/detailed SQL.
///
/// `None` means "no filter". Whitespace-only input collapses to `None`.
fn build_like_pattern(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)))
}

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
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(validate_ident)
            .transpose()?;

        let pager = Pager::new(cursor, self.config.page_size);
        let pattern = build_like_pattern(search.as_deref());
        let limit = i64::try_from(pager.limit()).unwrap_or(i64::MAX);
        let offset = i64::try_from(pager.offset()).unwrap_or(i64::MAX);

        if !detailed {
            let rows: Vec<String> = self
                .connection
                .fetch_scalar(
                    sqlx::query(BRIEF_SQL).bind(pattern.clone()).bind(limit).bind(offset),
                    database,
                )
                .await?;
            let (tables, next_cursor) = pager.finalize(rows);
            return Ok(ListTablesResponse {
                tables: TableEntries::Brief(tables),
                next_cursor,
            });
        }

        let rows = self
            .connection
            .fetch_json(
                sqlx::query(DETAILED_SQL).bind(pattern.clone()).bind(limit).bind(offset),
                database,
            )
            .await?;
        // The CTE wraps each row as a JSON object under column `entry`;
        // when the row is flattened to JSON by `RowExt::to_json`, the
        // column name becomes the key. Unwrap to the inner object.
        let entries: Vec<_> = rows
            .into_iter()
            .filter_map(|mut v| v.as_object_mut().and_then(|obj| obj.remove("entry")).or(Some(v)))
            .collect();
        let (entries, next_cursor) = finalize_detailed(&pager, entries);
        Ok(ListTablesResponse {
            tables: TableEntries::Detailed(entries),
            next_cursor,
        })
    }
}

/// Mirrors [`Pager::finalize`] for an owned `Vec<Value>` without cloning the trait bound.
fn finalize_detailed(pager: &Pager, mut items: Vec<serde_json::Value>) -> (Vec<serde_json::Value>, Option<Cursor>) {
    pager.finalize(std::mem::take(&mut items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_like_leaves_plain_text_unchanged() {
        assert_eq!(escape_like(""), "");
        assert_eq!(escape_like("orders"), "orders");
        assert_eq!(escape_like("Robert tables"), "Robert tables");
    }

    #[test]
    fn escape_like_escapes_percent() {
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("%%"), "\\%\\%");
    }

    #[test]
    fn escape_like_escapes_underscore() {
        assert_eq!(escape_like("user_id"), "user\\_id");
    }

    #[test]
    fn escape_like_escapes_backslash() {
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_like_escapes_all_meta_together() {
        assert_eq!(escape_like("100%_done\\"), "100\\%\\_done\\\\");
    }

    #[test]
    fn escape_like_passes_through_sql_meta_chars() {
        // Single quote, semicolon, double-dash are NOT LIKE meta — they must survive unchanged.
        // (They are rendered safe by sqlx parameter binding, not by this function.)
        assert_eq!(escape_like("Robert'; DROP TABLE --"), "Robert'; DROP TABLE --");
    }

    #[test]
    fn build_like_pattern_none_for_none_empty_whitespace() {
        assert_eq!(build_like_pattern(None), None);
        assert_eq!(build_like_pattern(Some("")), None);
        assert_eq!(build_like_pattern(Some("   ")), None);
        assert_eq!(build_like_pattern(Some("\t\n")), None);
    }

    #[test]
    fn build_like_pattern_wraps_and_escapes() {
        assert_eq!(build_like_pattern(Some("order")), Some("%order%".into()));
        assert_eq!(build_like_pattern(Some("100%")), Some("%100\\%%".into()));
        assert_eq!(build_like_pattern(Some("user_id")), Some("%user\\_id%".into()));
    }

    #[test]
    fn build_like_pattern_trims_surrounding_whitespace() {
        assert_eq!(build_like_pattern(Some("  order  ")), Some("%order%".into()));
    }
}
