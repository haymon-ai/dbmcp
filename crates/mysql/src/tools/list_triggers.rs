//! MCP tool: `listTriggers`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListTriggersRequest, ListTriggersResponse};
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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

impl AsyncTool<MysqlHandler> for ListTriggersTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_triggers(params).await
    }
}

/// Brief-mode SQL: name-only column with optional case-insensitive `LIKE` filter.
///
/// `CAST(TRIGGER_NAME AS CHAR)` forces a `VARCHAR` decode — `MySQL` 9 reports
/// `information_schema` text columns as `VARBINARY`. `LOWER(...)` on both sides
/// of the `LIKE` makes the match case-insensitive regardless of column collation.
const BRIEF_SQL: &str = r"
    SELECT CAST(TRIGGER_NAME AS CHAR)
    FROM information_schema.TRIGGERS
    WHERE TRIGGER_SCHEMA = ?
      AND (? IS NULL OR LOWER(TRIGGER_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TRIGGER_NAME
    LIMIT ? OFFSET ?";

/// Detailed-mode SQL — single SELECT against `information_schema.TRIGGERS`.
///
/// `JSON_OBJECT(...)` projects ten fields per row. Identifiers in the
/// reconstructed `definition` are backtick-quoted with embedded backticks
/// doubled. The `DEFINER` column stores `user@host` unquoted; the user
/// portion can itself contain `@` (e.g. `'foo@bar'@'localhost'`), so the
/// host is the segment after the **last** `@` and the user is everything
/// before it (`SUBSTRING_INDEX(..., '@', -1)` for host, `LEFT(...)` for
/// user). The five comma-separated `CONCAT` chunks rebuild the canonical
/// `SHOW CREATE TRIGGER` `DEFINER=` `` `<user>`@`<host>` `` opener inline
/// so identifier escaping and the last-`@` split live next to the rest of
/// the projection. `events` is always a single-element array on
/// `MySQL`/`MariaDB` (the engine fires one event per definition);
/// `activationLevel` is always `ROW`. `ORDER BY TRIGGER_NAME` is sufficient —
/// `(TRIGGER_SCHEMA, TRIGGER_NAME)` is the table's primary key, and the
/// `WHERE` clause already pins `TRIGGER_SCHEMA`.
const DETAILED_SQL: &str = r"
    SELECT
        CAST(TRIGGER_NAME AS CHAR) AS name,
        JSON_OBJECT(
            'schema',              CAST(EVENT_OBJECT_SCHEMA AS CHAR),
            'table',               CAST(EVENT_OBJECT_TABLE  AS CHAR),
            'timing',              CAST(ACTION_TIMING       AS CHAR),
            'events',              JSON_ARRAY(CAST(EVENT_MANIPULATION AS CHAR)),
            'activationLevel',     CAST(ACTION_ORIENTATION  AS CHAR),
            'definition',          CONCAT(
                'CREATE DEFINER=`',
                REPLACE(LEFT(DEFINER, LENGTH(DEFINER) - LENGTH(SUBSTRING_INDEX(DEFINER, '@', -1)) - 1), '`', '``'),
                '`@`',
                REPLACE(SUBSTRING_INDEX(DEFINER, '@', -1), '`', '``'),
                '`',
                ' TRIGGER ',
                '`', REPLACE(TRIGGER_NAME, '`', '``'), '`',
                ' ', ACTION_TIMING, ' ', EVENT_MANIPULATION,
                ' ON ',
                '`', REPLACE(EVENT_OBJECT_TABLE,  '`', '``'), '`',
                ' FOR EACH ROW ', ACTION_STATEMENT
            ),
            'sqlMode',             CAST(SQL_MODE                AS CHAR),
            'characterSetClient',  CAST(CHARACTER_SET_CLIENT    AS CHAR),
            'collationConnection', CAST(COLLATION_CONNECTION    AS CHAR),
            'databaseCollation',   CAST(DATABASE_COLLATION      AS CHAR)
        ) AS entry
    FROM information_schema.TRIGGERS
    WHERE TRIGGER_SCHEMA = ?
      AND (? IS NULL OR LOWER(TRIGGER_NAME) LIKE LOWER(CONCAT('%', ?, '%')))
    ORDER BY TRIGGER_NAME
    LIMIT ? OFFSET ?";

impl MysqlHandler {
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
            return Ok(ListTriggersResponse::detailed(
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
        let (triggers, next_cursor) = pager.paginate(rows);
        Ok(ListTriggersResponse::brief(triggers, next_cursor))
    }
}
