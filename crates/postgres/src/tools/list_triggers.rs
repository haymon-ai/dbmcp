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
    const DESCRIPTION: &'static str = include_str!("../../assets/tools/list_triggers.md");
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
