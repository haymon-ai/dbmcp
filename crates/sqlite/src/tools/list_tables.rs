//! MCP tool: `listTables`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::{ListTablesRequest, ListTablesResponse};

/// Marker type for the `listTables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "listTables";
    const TITLE: &'static str = "List Tables";
    const DESCRIPTION: &'static str = r#"List tables in the connected database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Exploring a database to find relevant tables (brief mode, default).
- Searching for a table by partial name (pass `search`).
- Inspecting a table's columns, constraints, indexes, and triggers before writing a query (pass `detailed: true`). This supersedes the legacy `getTableSchema` workflow.
</usecase>

<parameters>
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on table names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by table name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What tables are in this database?" → listTables()
✓ "Find the orders table" → listTables(search="order")
✓ "What columns does orders have?" → listTables(search="orders", detailed=true)
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of table-name strings, e.g. `["customers", "orders"]`.
Detailed mode: a JSON object keyed by table name; each value carries `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, and `triggers` (the name is the key and is not repeated inside the value). Keys iterate in the same alphabetical order brief mode uses. Example: `{"orders": {"schema": "main", "kind": "TABLE", ...}}`.
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

impl AsyncTool<SqliteHandler> for ListTablesTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_tables(params).await
    }
}

/// Brief-mode SQL: `pragma_table_list` scan with optional `LIKE` filter.
///
/// `pragma_table_list` distinguishes ordinary tables (`type = 'table'`),
/// virtual tables (`type = 'virtual'`), views (`type = 'view'`), and the
/// automatically-generated shadow tables that back FTS5 / R-Tree / etc.
/// (`type = 'shadow'`). Restricting to `('table', 'virtual')` hides shadow
/// tables from users while still surfacing user-declared virtual tables.
///
/// `SQLite`'s `LIKE` operator is case-insensitive for ASCII by default and
/// `dbmcp` does not toggle the `case_sensitive_like` PRAGMA, so a bare
/// `LIKE` already matches case-insensitively (a `COLLATE NOCASE` on the
/// RHS pattern would be a no-op — `LIKE` does not honor right-hand-side
/// collation; see <https://www.sqlite.org/lang_expr.html>). User-facing
/// wildcards (`%`, `_`) in `?1` flow straight into LIKE semantics — this
/// matches the `PostgreSQL` contract established in commit `dbe917f`.
const BRIEF_SQL: &str = r"
    SELECT tl.name
    FROM pragma_table_list tl
    WHERE tl.schema = 'main'
      AND tl.type IN ('table', 'virtual')
      AND tl.name NOT LIKE 'sqlite_%'
      AND (?1 IS NULL OR tl.name LIKE '%' || ?1 || '%')
    ORDER BY tl.name
    LIMIT ?2 OFFSET ?3";

/// Detailed-mode SQL: single CTE joining `sqlite_master` with `pragma_*`
/// table-valued functions and aggregating each table's columns, constraints,
/// indexes, and triggers into JSON via `json_object` / `json_group_array`.
///
/// `LIMIT`/`OFFSET` are pushed into `table_info` so every downstream CTE scans
/// at most `page_size + 1` tables, never the full schema.
///
/// `SQLite`-specific carve-outs (intentionally silent to clients per FR-012):
/// CHECK constraints are not exposed by `SQLite`'s catalog, so `constraints[]`
/// only emits PRIMARY KEY / FOREIGN KEY / UNIQUE entries. `owner`, `comment`,
/// and column-level `comment` are always `NULL`. `method` is always `btree`,
/// trigger `enabled` is always `true`, and `indexes[].definition` is either
/// the raw `sqlite_master.sql` or, for auto-generated indexes with no user
/// SQL, a synthesised `CREATE [UNIQUE] INDEX …` string so the field is never
/// null. Columns use 1-based `ordinalPosition` (`cid + 1`) for cross-backend
/// parity with `PostgreSQL`'s `pg_attribute.attnum`.
const DETAILED_SQL: &str = r#"
    WITH table_info AS (
        SELECT
            tl.name AS table_name,
            CASE tl.type
                WHEN 'virtual' THEN 'VIRTUAL_TABLE'
                ELSE 'TABLE'
            END AS kind
        FROM pragma_table_list tl
        WHERE tl.schema = 'main'
          AND tl.type IN ('table', 'virtual')
          AND tl.name NOT LIKE 'sqlite_%'
          AND (?1 IS NULL OR tl.name LIKE '%' || ?1 || '%')
        ORDER BY tl.name
        LIMIT ?2 OFFSET ?3
    ),
    columns_info AS (
        SELECT
            ti.table_name,
            c.cid AS cid,
            json_object(
                'name',            c.name,
                'dataType',        c.type,
                'ordinalPosition', c.cid + 1,
                'nullable',        json(CASE WHEN c."notnull" = 0 THEN 'true' ELSE 'false' END),
                'default',         c.dflt_value,
                'comment',         NULL
            ) AS column_json
        FROM table_info ti, pragma_table_info(ti.table_name) c
    ),
    pk_constraints AS (
        SELECT
            ti.table_name,
            json_object(
                'name',       'PRIMARY',
                'type',       'PRIMARY KEY',
                'columns',    json_group_array(c.name),
                'definition', 'PRIMARY KEY (' || group_concat('"' || c.name || '"', ', ') || ')'
            ) AS constraint_json
        FROM table_info ti, pragma_table_info(ti.table_name) c
        WHERE c.pk > 0
        GROUP BY ti.table_name
        HAVING COUNT(c.name) > 0
    ),
    fk_constraints AS (
        SELECT
            ti.table_name,
            json_object(
                'name',              'fk_' || ti.table_name || '_' || f.id,
                'type',              'FOREIGN KEY',
                'columns',           json_group_array(f."from"),
                'definition',        'FOREIGN KEY (' || group_concat('"' || f."from" || '"', ', ')
                                     || ') REFERENCES "' || f."table" || '"('
                                     || group_concat('"' || f."to" || '"', ', ') || ')',
                'referencedTable',   f."table",
                'referencedColumns', json_group_array(f."to")
            ) AS constraint_json
        FROM table_info ti, pragma_foreign_key_list(ti.table_name) f
        GROUP BY ti.table_name, f.id
    ),
    unique_constraints AS (
        SELECT
            ti.table_name,
            json_object(
                'name',       i.name,
                'type',       'UNIQUE',
                'columns',    (SELECT json_group_array(ii.name)
                               FROM pragma_index_info(i.name) ii),
                'definition', 'UNIQUE ('
                              || (SELECT group_concat('"' || ii.name || '"', ', ')
                                  FROM pragma_index_info(i.name) ii)
                              || ')'
            ) AS constraint_json
        FROM table_info ti, pragma_index_list(ti.table_name) i
        WHERE i."unique" = 1 AND i.origin <> 'pk'
    ),
    all_constraints AS (
        SELECT table_name, constraint_json FROM pk_constraints
        UNION ALL
        SELECT table_name, constraint_json FROM fk_constraints
        UNION ALL
        SELECT table_name, constraint_json FROM unique_constraints
    ),
    indexes_info AS (
        SELECT
            ti.table_name,
            json_object(
                'name',       il.name,
                'columns',    (SELECT json_group_array(ii.name) FROM pragma_index_info(il.name) ii),
                'unique',     json(CASE WHEN il."unique" = 1 THEN 'true' ELSE 'false' END),
                'primary',    json(CASE WHEN il.origin = 'pk' THEN 'true' ELSE 'false' END),
                'method',     'btree',
                'definition', COALESCE(
                    (SELECT m2.sql FROM sqlite_master m2 WHERE m2.type = 'index' AND m2.name = il.name),
                    'CREATE ' || CASE il."unique" WHEN 1 THEN 'UNIQUE INDEX ' ELSE 'INDEX ' END
                        || '"' || il.name || '" ON "' || ti.table_name || '"('
                        || (SELECT group_concat('"' || ii.name || '"', ', ') FROM pragma_index_info(il.name) ii)
                        || ')'
                )
            ) AS index_json
        FROM table_info ti, pragma_index_list(ti.table_name) il
    ),
    triggers_info AS (
        SELECT
            m.tbl_name AS table_name,
            json_object(
                'name',       m.name,
                'definition', m.sql,
                'enabled',    json('true')
            ) AS trigger_json
        FROM sqlite_master m
        JOIN table_info ti ON ti.table_name = m.tbl_name
        WHERE m.type = 'trigger'
    )
    SELECT
        ti.table_name AS name,
        json_object(
            'schema',      'main',
            'kind',        ti.kind,
            'owner',       NULL,
            'comment',     NULL,
            'columns',     COALESCE(
                               (SELECT json_group_array(json(ci.column_json))
                                FROM (SELECT column_json
                                      FROM columns_info
                                      WHERE table_name = ti.table_name
                                      ORDER BY cid) ci),
                               json('[]')),
            'constraints', COALESCE(
                               (SELECT json_group_array(json(ac.constraint_json))
                                FROM all_constraints ac
                                WHERE ac.table_name = ti.table_name),
                               json('[]')),
            'indexes',     COALESCE(
                               (SELECT json_group_array(json(ii.index_json))
                                FROM indexes_info ii
                                WHERE ii.table_name = ti.table_name),
                               json('[]')),
            'triggers',    COALESCE(
                               (SELECT json_group_array(json(tg.trigger_json))
                                FROM triggers_info tg
                                WHERE tg.table_name = ti.table_name),
                               json('[]'))
        ) AS entry
    FROM table_info ti
    ORDER BY ti.table_name"#;

impl SqliteHandler {
    /// Lists one page of tables in the connected database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed, or
    /// an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest {
            cursor,
            search,
            detailed,
        }: ListTablesRequest,
    ) -> Result<ListTablesResponse, ErrorData> {
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
                    None,
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
                None,
            )
            .await?;
        let (tables, next_cursor) = pager.paginate(rows);
        Ok(ListTablesResponse::brief(tables, next_cursor))
    }
}
