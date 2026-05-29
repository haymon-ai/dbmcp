//! MCP tool: `listViews`.

use dbmcp_server::pagination::{Cursor, Pager};

use super::prelude::*;
use crate::types::{ListEntriesResponse, PinnedListViewsRequest, UnpinnedListViewsRequest};

const NAME: &str = "listViews";
const TITLE: &str = "List Views";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/list_views/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/list_views/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// Marker type for the `listViews` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedListViewsTool;

impl ToolBase for PinnedListViewsTool {
    type Parameter = PinnedListViewsRequest;
    type Output = ListEntriesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_PINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for PinnedListViewsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_views(None, params.cursor, params.search, params.detailed)
            .await
    }
}

/// Marker type for the `listViews` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedListViewsTool;

impl ToolBase for UnpinnedListViewsTool {
    type Parameter = UnpinnedListViewsRequest;
    type Output = ListEntriesResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_UNPINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for UnpinnedListViewsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_views(
                params.database,
                params.inner.cursor,
                params.inner.search,
                params.inner.detailed,
            )
            .await
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
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
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
            return Ok(ListEntriesResponse::detailed(
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

        Ok(ListEntriesResponse::brief(views, next_cursor))
    }
}
