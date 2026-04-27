//! MCP tool: `listTriggers`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::ListTriggersResponse;

use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ListTriggersRequest;

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ListTriggersTool {
    const NAME: &'static str = "listTriggers";
    const TITLE: &'static str = "List Triggers";
    const DESCRIPTION: &'static str = r#"List triggers in the connected SQLite database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Auditing trigger coverage across a database (brief mode, default).
- Searching for a trigger by partial name (pass `search`).
- Inspecting a trigger's table and full `CREATE TRIGGER` text before reasoning about side-effects (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `sqlite_schema`.
</usecase>

<parameters>
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on trigger names via `LIKE` (SQLite's `LIKE` is ASCII-case-insensitive by default). `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by trigger name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What triggers are in this database?" → listTriggers()
✓ "Find the audit triggers" → listTriggers(search="audit")
✓ "What does orders_audit_after_insert do?" → listTriggers(search="orders_audit_after_insert", detailed=true)
✗ "Show me a trigger's body" → use detailed mode; the `definition` field carries the full `CREATE TRIGGER` text
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of trigger-name strings, e.g. `["customers_audit_after_insert", "orders_audit_after_insert"]`.
Detailed mode: a JSON object keyed by trigger name; each value carries exactly three fields — `schema` (always `"main"`), `table` (`sqlite_schema.tbl_name` — may be a view name for `INSTEAD OF` triggers), and `definition` (the original `CREATE TRIGGER` text from `sqlite_schema.sql`, byte-for-byte). Internal `sqlite_*` triggers are excluded.
The detailed payload deliberately diverges from the Postgres and MySQL/MariaDB `listTriggers` detailed payloads — `timing`, `events`, `activationLevel`, `status`, `functionName`, `sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`, and `created` are absent. SQLite's catalogue does not expose those concepts as columns, and this tool deliberately avoids parsing the stored DDL to derive them; clients that need the timing or event keyword can read it off the prefix of `definition`.
Triggers whose stored `sqlite_schema.sql` is `NULL` (rare; produced by extension-generated rows or hand-edited catalogues) are silently omitted from detailed mode but still listed by name in brief mode.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>"#;
}

impl ToolBase for ListTriggersTool {
    type Parameter = ListTriggersRequest;
    type Output = ListTriggersResponse;
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

impl AsyncTool<SqliteHandler> for ListTriggersTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_triggers(params).await
    }
}

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `SQLite`'s `LIKE` operator is case-insensitive for ASCII by default and
/// `dbmcp` does not toggle the `case_sensitive_like` PRAGMA, so a bare
/// `LIKE` already matches case-insensitively. (A `COLLATE NOCASE` clause on
/// the right-hand pattern would be a no-op — `LIKE` does not honor RHS
/// collation; see <https://www.sqlite.org/lang_expr.html>.) User-facing
/// `LIKE` wildcards (`%`, `_`) in `?1` flow straight through.
const BRIEF_SQL: &str = r"
    SELECT name
    FROM sqlite_schema
    WHERE type = 'trigger'
      AND name NOT LIKE 'sqlite_%'
      AND (?1 IS NULL OR name LIKE '%' || ?1 || '%')
    ORDER BY name
    LIMIT ?2 OFFSET ?3";

/// Detailed-mode SQL: single SELECT projecting `(name, json_object(...))`.
///
/// `'main'` is hard-coded in the projection — `sqlite_schema` here is the
/// `main` schema's catalogue, the only one the connection helper opens.
/// `AND sql IS NOT NULL` enforces the homogeneous-shape contract: rows whose
/// stored `CREATE TRIGGER` text is `NULL` (extension-generated, hand-edited)
/// are silently omitted from detailed pages. Brief mode does not filter on
/// `sql` so those triggers are still discoverable by name. `SQLite` trigger
/// names are unique within a database, so `ORDER BY name` is sufficient
/// for stable pagination — no secondary tie-breaker needed.
const DETAILED_SQL: &str = r"
    SELECT
        name,
        json_object(
            'schema',     'main',
            'table',      tbl_name,
            'definition', sql
        ) AS entry
    FROM sqlite_schema
    WHERE type = 'trigger'
      AND name NOT LIKE 'sqlite_%'
      AND sql IS NOT NULL
      AND (?1 IS NULL OR name LIKE '%' || ?1 || '%')
    ORDER BY name
    LIMIT ?2 OFFSET ?3";

impl SqliteHandler {
    /// Lists one page of triggers in the connected database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest {
            cursor,
            search,
            detailed,
        }: ListTriggersRequest,
    ) -> Result<ListTriggersResponse, ErrorData> {
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
            return Ok(ListTriggersResponse::detailed(
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
        let (triggers, next_cursor) = pager.paginate(rows);
        Ok(ListTriggersResponse::brief(triggers, next_cursor))
    }
}
