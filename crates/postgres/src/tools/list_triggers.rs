//! MCP tool: `listTriggers`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::{ListTriggersRequest, ListTriggersResponse};

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ListTriggersTool {
    const NAME: &'static str = "listTriggers";
    const TITLE: &'static str = "List Triggers";
    const DESCRIPTION: &'static str = r#"List user-defined triggers in the `public` schema, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Auditing triggers across a database (brief mode, default).
- Searching for a trigger by partial name (pass `search`).
- Inspecting a trigger's timing, events, activation level, handler function, status, and full `CREATE TRIGGER` text before reasoning about side-effects (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_trigger` / `information_schema.triggers`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on trigger names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by trigger name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What triggers are in the mydb database?" → listTriggers(database="mydb")
✓ "Find the audit triggers" → listTriggers(search="audit")
✓ "What does orders_audit_after_iu do?" → listTriggers(search="orders_audit_after_iu", detailed=true)
✗ "Show me a trigger's body" → use detailed mode; the `definition` field carries the full `CREATE TRIGGER` text
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of trigger-name strings, e.g. `["customers_audit_after_insert", "orders_audit_after_insert"]`.
Detailed mode: a JSON object keyed by trigger name; each value carries `schema`, `table`, `status` (ENABLED/DISABLED/REPLICA/ALWAYS), `timing` (BEFORE/AFTER/INSTEAD OF), `events` (array of strings drawn from INSERT/UPDATE/DELETE/TRUNCATE in that fixed order), `activationLevel` (ROW/STATEMENT), `functionName`, and `definition` (the full `CREATE TRIGGER` text). Internal triggers (FK enforcement etc.) are excluded.
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

impl AsyncTool<PostgresHandler> for ListTriggersTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_triggers(params).await
    }
}

/// Brief-mode SQL: `pg_trigger` join with optional `ILIKE` filter on trigger name.
///
/// `'public'::regnamespace` casts the schema name to a `pg_namespace.oid`
/// at plan time, replacing what would otherwise be a `pg_namespace` join.
/// Trigger names are not globally unique on Postgres — the same trigger
/// name can appear on multiple relations — so `c.relname` is the secondary
/// sort key, matching the detailed-mode `ORDER BY` and keeping `OFFSET`
/// pagination stable across duplicate names.
const BRIEF_SQL: &str = r"
    SELECT t.tgname
    FROM pg_trigger t
    JOIN pg_class c ON t.tgrelid = c.oid
    WHERE c.relnamespace = 'public'::regnamespace
      AND NOT t.tgisinternal
      AND ($1::text IS NULL OR t.tgname ILIKE '%' || $1 || '%')
    ORDER BY t.tgname, c.relname
    LIMIT $2 OFFSET $3";

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
const DETAILED_SQL: &str = r"
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

impl PostgresHandler {
    /// Lists one page of user-defined triggers, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListTriggersRequest,
    ) -> Result<ListTriggersResponse, ErrorData> {
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
                database,
            )
            .await?;
        let (triggers, next_cursor) = pager.paginate(rows);
        Ok(ListTriggersResponse::brief(triggers, next_cursor))
    }
}
