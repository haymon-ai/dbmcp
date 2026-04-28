//! MCP tool: `listTables`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::types::{ListTablesRequest, ListTablesResponse};

/// Brief-mode SQL: `information_schema.TABLES` filtered to `BASE TABLE` rows.
///
/// `CAST(... AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`, which the `String`
/// decoder rejects. `LOWER(...)` on both sides of `LIKE` makes the match
/// case-insensitive regardless of column collation (`MySQL` 9 reports
/// `TABLE_NAME` as `VARBINARY` — binary collation is case-sensitive).
const BRIEF_SQL: &str = r"
    SELECT CAST(TABLE_NAME AS CHAR)
    FROM information_schema.TABLES
    WHERE TABLE_SCHEMA = ?
      AND TABLE_TYPE = 'BASE TABLE'
      AND (? IS NULL OR LOWER(TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TABLE_NAME
    LIMIT ? OFFSET ?";

/// Detailed-mode SQL: per-table `JSON_OBJECT` projection over a CTE chain.
///
/// Bind order: `database`, `pattern`, `pattern`, `limit`, `offset`, `database`.
/// `MySQL`/`MariaDB` lack `QUOTE_IDENT`; backtick-quoting is inlined per identifier.
/// `MySQL` 9 rejects inline `ORDER BY` in `JSON_ARRAYAGG`. `CAST(... AS JSON)` is
/// avoided because `MariaDB` rejects it (its `JSON` type is a `LONGTEXT` alias,
/// not a CAST target); `JSON_EXTRACT(text, '$')` re-parses to JSON on both.
/// `JSON_QUOTE` inside `GROUP_CONCAT` returns empty on `MySQL` 9 (`information_schema`
/// text columns are `VARBINARY`-typed), so column names are wrapped via plain
/// `CONCAT('"', x, '"')` — safe because identifiers cannot contain `"` or `\`.
/// JSON booleans are produced via `JSON_EXTRACT(IF(cond, 'true', 'false'), '$')`.
/// The `triggers_info` CTE renders the `DEFINER` clause in canonical
/// `SHOW CREATE TRIGGER` form with each component backtick-quoted. The
/// `DEFINER` column stores `user@host` unquoted and `user` may itself
/// contain `@` (e.g. `'foo@bar'@'localhost'`), so the host is the
/// segment after the **last** `@` (`SUBSTRING_INDEX(..., '@', -1)`) and the
/// user is everything before it (`LEFT(..., LENGTH - host_len - 1)`), with
/// embedded backticks doubled in both components.
const DETAILED_SQL: &str = r#"
WITH table_info AS (
    SELECT
        t.TABLE_SCHEMA AS table_schema,
        t.TABLE_NAME   AS table_name,
        NULLIF(t.TABLE_COMMENT, '') AS table_comment
    FROM information_schema.TABLES t
    WHERE t.TABLE_SCHEMA = ?
      AND t.TABLE_TYPE   = 'BASE TABLE'
      AND (? IS NULL OR LOWER(t.TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY t.TABLE_NAME
    LIMIT ? OFFSET ?
),
partitions_info AS (
    SELECT
        p.TABLE_SCHEMA,
        p.TABLE_NAME,
        MAX(p.PARTITION_METHOD IS NOT NULL) AS has_partitions
    FROM information_schema.PARTITIONS p
    WHERE p.TABLE_SCHEMA = ?
    GROUP BY p.TABLE_SCHEMA, p.TABLE_NAME
),
columns_info AS (
    SELECT
        c.TABLE_SCHEMA,
        c.TABLE_NAME,
        c.COLUMN_NAME      AS column_name,
        c.COLUMN_TYPE      AS data_type,
        c.ORDINAL_POSITION AS ordinal_position,
        (c.IS_NULLABLE = 'YES') AS nullable,
        IF(c.EXTRA LIKE '%GENERATED%', c.GENERATION_EXPRESSION, c.COLUMN_DEFAULT) AS column_default,
        NULLIF(c.COLUMN_COMMENT, '') AS column_comment
    FROM information_schema.COLUMNS c
    JOIN table_info ti
      ON ti.table_schema = c.TABLE_SCHEMA
     AND ti.table_name   = c.TABLE_NAME
),
constraints_info AS (
    SELECT
        tc.TABLE_SCHEMA,
        tc.TABLE_NAME,
        tc.CONSTRAINT_NAME AS name,
        tc.CONSTRAINT_TYPE AS type,
        JSON_EXTRACT(CONCAT('[', IFNULL(GROUP_CONCAT(CONCAT('"', kcu.COLUMN_NAME, '"') ORDER BY kcu.ORDINAL_POSITION SEPARATOR ','), ''), ']'), '$') AS columns,
        CONCAT(
            IF(tc.CONSTRAINT_TYPE = 'PRIMARY KEY', 'PRIMARY KEY (', 'UNIQUE ('),
            GROUP_CONCAT(
                CONCAT('`', REPLACE(kcu.COLUMN_NAME, '`', '``'), '`')
                ORDER BY kcu.ORDINAL_POSITION SEPARATOR ', '
            ),
            ')'
        ) AS definition,
        CAST(NULL AS CHAR)  AS referenced_table,
        CAST(NULL AS CHAR)  AS referenced_columns
    FROM information_schema.TABLE_CONSTRAINTS tc
    JOIN information_schema.KEY_COLUMN_USAGE kcu
      ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
     AND kcu.CONSTRAINT_NAME   = tc.CONSTRAINT_NAME
     AND kcu.TABLE_SCHEMA      = tc.TABLE_SCHEMA
     AND kcu.TABLE_NAME        = tc.TABLE_NAME
    JOIN table_info ti
      ON ti.table_schema = tc.TABLE_SCHEMA
     AND ti.table_name   = tc.TABLE_NAME
    WHERE tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE')
    GROUP BY tc.TABLE_SCHEMA, tc.TABLE_NAME, tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE
    UNION ALL
    SELECT
        tc.TABLE_SCHEMA,
        tc.TABLE_NAME,
        tc.CONSTRAINT_NAME AS name,
        'FOREIGN KEY'      AS type,
        JSON_EXTRACT(CONCAT('[', IFNULL(GROUP_CONCAT(CONCAT('"', kcu.COLUMN_NAME, '"') ORDER BY kcu.ORDINAL_POSITION SEPARATOR ','), ''), ']'), '$') AS columns,
        CONCAT(
            'FOREIGN KEY (',
            GROUP_CONCAT(
                CONCAT('`', REPLACE(kcu.COLUMN_NAME, '`', '``'), '`')
                ORDER BY kcu.ORDINAL_POSITION SEPARATOR ', '
            ),
            ') REFERENCES ',
            CONCAT('`', REPLACE(MAX(kcu.REFERENCED_TABLE_NAME), '`', '``'), '`'),
            '(',
            GROUP_CONCAT(
                CONCAT('`', REPLACE(kcu.REFERENCED_COLUMN_NAME, '`', '``'), '`')
                ORDER BY kcu.ORDINAL_POSITION SEPARATOR ', '
            ),
            ') ON UPDATE ', MAX(rc.UPDATE_RULE),
            ' ON DELETE ',  MAX(rc.DELETE_RULE)
        ) AS definition,
        MAX(kcu.REFERENCED_TABLE_NAME) AS referenced_table,
        JSON_EXTRACT(CONCAT('[', IFNULL(GROUP_CONCAT(CONCAT('"', kcu.REFERENCED_COLUMN_NAME, '"') ORDER BY kcu.ORDINAL_POSITION SEPARATOR ','), ''), ']'), '$') AS referenced_columns
    FROM information_schema.TABLE_CONSTRAINTS tc
    JOIN information_schema.KEY_COLUMN_USAGE kcu
      ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
     AND kcu.CONSTRAINT_NAME   = tc.CONSTRAINT_NAME
     AND kcu.TABLE_SCHEMA      = tc.TABLE_SCHEMA
     AND kcu.TABLE_NAME        = tc.TABLE_NAME
    JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
      ON rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
     AND rc.CONSTRAINT_NAME   = tc.CONSTRAINT_NAME
    JOIN table_info ti
      ON ti.table_schema = tc.TABLE_SCHEMA
     AND ti.table_name   = tc.TABLE_NAME
    WHERE tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
    GROUP BY tc.TABLE_SCHEMA, tc.TABLE_NAME, tc.CONSTRAINT_NAME
    UNION ALL
    SELECT
        cc.CONSTRAINT_SCHEMA AS TABLE_SCHEMA,
        tc.TABLE_NAME,
        cc.CONSTRAINT_NAME   AS name,
        'CHECK'              AS type,
        JSON_ARRAY()         AS columns,
        cc.CHECK_CLAUSE      AS definition,
        CAST(NULL AS CHAR)   AS referenced_table,
        CAST(NULL AS CHAR)   AS referenced_columns
    FROM information_schema.CHECK_CONSTRAINTS cc
    JOIN information_schema.TABLE_CONSTRAINTS tc
      ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA
     AND tc.CONSTRAINT_NAME   = cc.CONSTRAINT_NAME
    JOIN table_info ti
      ON ti.table_schema = tc.TABLE_SCHEMA
     AND ti.table_name   = tc.TABLE_NAME
),
indexes_info AS (
    SELECT
        s.TABLE_SCHEMA,
        s.TABLE_NAME,
        s.INDEX_NAME AS index_name,
        GROUP_CONCAT(
            CASE
                WHEN s.SUB_PART IS NOT NULL THEN CONCAT('`', REPLACE(s.COLUMN_NAME, '`', '``'), '`', '(', s.SUB_PART, ')')
                ELSE CONCAT('`', REPLACE(s.COLUMN_NAME, '`', '``'), '`')
            END
            ORDER BY s.SEQ_IN_INDEX SEPARATOR ', '
        ) AS definition_cols,
        JSON_EXTRACT(CONCAT('[', IFNULL(GROUP_CONCAT(CONCAT('"', s.COLUMN_NAME, '"') ORDER BY s.SEQ_IN_INDEX SEPARATOR ','), ''), ']'), '$') AS columns,
        (MIN(s.NON_UNIQUE) = 0)        AS is_unique,
        (s.INDEX_NAME = 'PRIMARY')     AS is_primary,
        LOWER(MIN(s.INDEX_TYPE))       AS method,
        MIN(s.INDEX_TYPE)              AS index_type_raw
    FROM information_schema.STATISTICS s
    JOIN table_info ti
      ON ti.table_schema = s.TABLE_SCHEMA
     AND ti.table_name   = s.TABLE_NAME
    GROUP BY s.TABLE_SCHEMA, s.TABLE_NAME, s.INDEX_NAME
),
triggers_info AS (
    SELECT
        tr.EVENT_OBJECT_SCHEMA AS TABLE_SCHEMA,
        tr.EVENT_OBJECT_TABLE  AS TABLE_NAME,
        tr.TRIGGER_NAME        AS trigger_name,
        CONCAT(
            'CREATE DEFINER=`',
            REPLACE(LEFT(tr.DEFINER, LENGTH(tr.DEFINER) - LENGTH(SUBSTRING_INDEX(tr.DEFINER, '@', -1)) - 1), '`', '``'),
            '`@`',
            REPLACE(SUBSTRING_INDEX(tr.DEFINER, '@', -1), '`', '``'),
            '`',
            ' TRIGGER ', '`', REPLACE(tr.TRIGGER_NAME, '`', '``'), '`',
            ' ', tr.ACTION_TIMING, ' ', tr.EVENT_MANIPULATION,
            ' ON ',
            '`', REPLACE(tr.EVENT_OBJECT_SCHEMA, '`', '``'), '`',
            '.',
            '`', REPLACE(tr.EVENT_OBJECT_TABLE, '`', '``'), '`',
            ' FOR EACH ROW ', tr.ACTION_STATEMENT
        ) AS definition
    FROM information_schema.TRIGGERS tr
    JOIN table_info ti
      ON ti.table_schema = tr.EVENT_OBJECT_SCHEMA
     AND ti.table_name   = tr.EVENT_OBJECT_TABLE
)
SELECT
    CAST(ti.table_name AS CHAR) AS name,
    JSON_OBJECT(
        'schema',  ti.table_schema,
        'kind',    IF(COALESCE(pi.has_partitions, FALSE), 'PARTITIONED_TABLE', 'TABLE'),
        'owner',   NULL,
        'comment', ti.table_comment,
        'columns', COALESCE((
            SELECT JSON_ARRAYAGG(JSON_OBJECT(
                'name',            ci.column_name,
                'dataType',        ci.data_type,
                'ordinalPosition', ci.ordinal_position,
                'nullable',        JSON_EXTRACT(IF(ci.nullable = 1, 'true', 'false'), '$'),
                'default',         ci.column_default,
                'comment',         ci.column_comment
            ))
            FROM columns_info ci
            WHERE ci.TABLE_SCHEMA = ti.table_schema AND ci.TABLE_NAME = ti.table_name
        ), JSON_ARRAY()),
        'constraints', COALESCE((
            SELECT JSON_ARRAYAGG(JSON_OBJECT(
                'name',              co.name,
                'type',              co.type,
                'columns',           JSON_EXTRACT(co.columns, '$'),
                'definition',        co.definition,
                'referencedTable',   co.referenced_table,
                'referencedColumns', JSON_EXTRACT(co.referenced_columns, '$')
            ))
            FROM constraints_info co
            WHERE co.TABLE_SCHEMA = ti.table_schema AND co.TABLE_NAME = ti.table_name
        ), JSON_ARRAY()),
        'indexes', COALESCE((
            SELECT JSON_ARRAYAGG(JSON_OBJECT(
                'name',       ii.index_name,
                'columns',    JSON_EXTRACT(ii.columns, '$'),
                'unique',     JSON_EXTRACT(IF(ii.is_unique = 1, 'true', 'false'), '$'),
                'primary',    JSON_EXTRACT(IF(ii.is_primary = 1, 'true', 'false'), '$'),
                'method',     ii.method,
                'definition', IF(ii.is_primary = 1,
                    CONCAT('PRIMARY KEY (', ii.definition_cols, ') USING ', ii.index_type_raw),
                    CONCAT(
                        'CREATE ',
                        CASE
                            WHEN ii.is_unique = 1                THEN 'UNIQUE '
                            WHEN ii.index_type_raw = 'FULLTEXT'  THEN 'FULLTEXT '
                            WHEN ii.index_type_raw = 'SPATIAL'   THEN 'SPATIAL '
                            ELSE ''
                        END,
                        'INDEX ', '`', REPLACE(ii.index_name, '`', '``'), '`',
                        ' ON ',
                        '`', REPLACE(ti.table_schema, '`', '``'), '`',
                        '.',
                        '`', REPLACE(ti.table_name, '`', '``'), '`',
                        '(', ii.definition_cols, ') USING ', ii.index_type_raw
                    )
                )
            ))
            FROM indexes_info ii
            WHERE ii.TABLE_SCHEMA = ti.table_schema AND ii.TABLE_NAME = ti.table_name
        ), JSON_ARRAY()),
        'triggers', COALESCE((
            SELECT JSON_ARRAYAGG(JSON_OBJECT(
                'name',       tri.trigger_name,
                'definition', tri.definition,
                'enabled',    JSON_EXTRACT('true', '$')
            ))
            FROM triggers_info tri
            WHERE tri.TABLE_SCHEMA = ti.table_schema AND tri.TABLE_NAME = ti.table_name
        ), JSON_ARRAY())
    ) AS entry
FROM table_info ti
LEFT JOIN partitions_info pi
  ON pi.TABLE_SCHEMA = ti.table_schema
 AND pi.TABLE_NAME   = ti.table_name
ORDER BY ti.table_name"#;

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
- `search` — Case-insensitive filter on table names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by table name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What tables are in mydb?" → listTables(database="mydb")
✓ "Find the orders table" → listTables(search="order")
✓ "What columns does orders have?" → listTables(search="orders", detailed=true)
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of table-name strings, e.g. `["customers", "orders"]`.
Detailed mode: a JSON object keyed by table name; each value carries `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, and `triggers` (the name is the key and is not repeated inside the value). Keys iterate in the same alphabetical order brief mode uses. Example: `{"orders": {"schema": "mydb", "kind": "TABLE", …}}`.
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

impl AsyncTool<MysqlHandler> for ListTablesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_tables(params).await
    }
}

impl MysqlHandler {
    /// Lists one page of tables in a database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed, or an
    /// internal-error [`ErrorData`] if `database` is invalid or the underlying
    /// query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListTablesRequest,
    ) -> Result<ListTablesResponse, ErrorData> {
        let database = validate_ident(
            database
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.connection.default_database_name()),
        )?;

        let pattern = search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let pager = Pager::new(cursor, self.config.page_size);

        if detailed {
            let rows: Vec<(String, sqlx::types::Json<serde_json::Value>)> = self
                .connection
                .fetch(
                    sqlx::query(DETAILED_SQL)
                        .bind(database)
                        .bind(pattern)
                        .bind(pattern)
                        .bind(pager.limit())
                        .bind(pager.offset())
                        .bind(database),
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
                    .bind(database)
                    .bind(pattern)
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
