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
