//! MCP tool: `listViews`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::types::{ListViewsRequest, ListViewsResponse};

/// Marker type for the `listViews` MCP tool.
pub(crate) struct ListViewsTool;

impl ListViewsTool {
    const NAME: &'static str = "listViews";
    const TITLE: &'static str = "List Views";
    const DESCRIPTION: &'static str = r#"List user-defined views in a database, optionally filtered and/or with full metadata. Base tables and system-schema views are excluded.

<usecase>
Use when:
- Auditing views across a database (brief mode, default).
- Searching for a view by partial name (pass `search`).
- Inspecting a view's definer, security mode, check-option level, updatable flag, session character set/collation, and full SELECT body before querying it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.VIEWS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on view names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by view name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What views are in the mydb database?" → listViews(database="mydb")
✓ "Find the active-users view" → listViews(search="active")
✓ "What does active_users select?" → listViews(search="active_users", detailed=true)
✗ "Show me the columns of a view" → use listTables with `detailed: true` instead
✗ "List materialized views" → MySQL/MariaDB have no materialized-view concept
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of view-name strings, e.g. `["active_orders", "active_users", "published_posts"]`.
Detailed mode: a JSON object keyed by bare view name; each value carries `schema`, `definer` (`user@host`), `security` (`INVOKER` or `DEFINER`), `checkOption` (`NONE`, `CASCADED`, or `LOCAL`), `updatable` (boolean), `characterSetClient`, `collationConnection`, and `definition` (the SELECT body verbatim from `information_schema.VIEWS.VIEW_DEFINITION`, with no `CREATE VIEW` wrapper). The view name is the map key only — it is not repeated inside the value.

Versus the Postgres `listViews` detailed payload: `description` is intentionally absent (neither MySQL nor MariaDB exposes a user-comment column for views — `CREATE VIEW` syntax has no `COMMENT` clause), `algorithm` is intentionally absent (MariaDB-only column on `information_schema.VIEWS`), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), and the five MySQL/MariaDB-only structured fields (`security`, `checkOption`, `updatable`, `characterSetClient`, `collationConnection`) are added. The `definition` field shape is byte-identical to Postgres — raw SELECT body verbatim, no DDL wrapper. When the connected role lacks the `SHOW VIEW` privilege on a particular view, the engine redacts `VIEW_DEFINITION` to the empty string; the row remains in the response with `definition` reflecting that empty value.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `TABLE_NAME` row order, so a client can switch `detailed` between pages without losing position.
</pagination>"#;
}

impl ToolBase for ListViewsTool {
    type Parameter = ListViewsRequest;
    type Output = ListViewsResponse;
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

impl AsyncTool<MysqlHandler> for ListViewsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_views(params).await
    }
}

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(TABLE_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
/// `(? IS NULL OR ...)` lets one prepared statement cover both filtered and
/// unfiltered cases.
const BRIEF_SQL: &str = r"
    SELECT CAST(TABLE_NAME AS CHAR)
    FROM information_schema.VIEWS
    WHERE TABLE_SCHEMA = ?
      AND (? IS NULL OR LOWER(TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TABLE_NAME
    LIMIT ? OFFSET ?";

/// Detailed-mode SQL — single SELECT against `information_schema.VIEWS`.
///
/// Eight fields per row, every value a single-column projection from the same
/// `information_schema.VIEWS` row. **No correlated subquery** (views have no
/// parameters). **No DDL reconstruction** (`VIEW_DEFINITION` already returns
/// the SELECT body verbatim — see research Decision 5). The `ALGORITHM` column
/// is deliberately not selected because it is a MariaDB-only addition; touching
/// it would fail on `MySQL` 9 (FR-006).
///
/// `LIMIT` pushes down before the JSON projection, so per-page work scales
/// with `page_size + 1` rows regardless of how many views the schema holds.
const DETAILED_SQL: &str = r"
    SELECT
        CAST(v.TABLE_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(v.TABLE_SCHEMA          AS CHAR),
            'definer',             CAST(v.DEFINER               AS CHAR),
            'security',            CAST(v.SECURITY_TYPE         AS CHAR),
            'checkOption',         CAST(v.CHECK_OPTION          AS CHAR),
            'updatable',           (v.IS_UPDATABLE = 'YES'),
            'characterSetClient',  CAST(v.CHARACTER_SET_CLIENT  AS CHAR),
            'collationConnection', CAST(v.COLLATION_CONNECTION  AS CHAR),
            'definition',          CAST(v.VIEW_DEFINITION       AS CHAR)
        ) AS entry
    FROM information_schema.VIEWS v
    WHERE v.TABLE_SCHEMA = ?
      AND (? IS NULL OR LOWER(v.TABLE_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY v.TABLE_NAME
    LIMIT ? OFFSET ?";

impl MysqlHandler {
    /// Lists one page of views, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest {
            database,
            cursor,
            search,
            detailed,
        }: ListViewsRequest,
    ) -> Result<ListViewsResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.connection.default_database_name());

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
                        .bind(pager.offset()),
                    None,
                )
                .await?;
            let (rows, next_cursor) = pager.paginate(rows);
            return Ok(ListViewsResponse::detailed(
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
        let (views, next_cursor) = pager.paginate(rows);

        Ok(ListViewsResponse::brief(views, next_cursor))
    }
}
