//! MCP tool: `listTriggers`.

use std::borrow::Cow;
use std::sync::Arc;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::ListTriggersResponse;

use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ListTriggersRequest;

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ListTriggersTool {
    const NAME: &'static str = "listTriggers";
    const TITLE: &'static str = "List Triggers";
    const DESCRIPTION: &'static str = r#"List all triggers in the connected SQLite database.

<usecase>
Use when:
- Investigating side-effects on INSERT/UPDATE/DELETE for a table
- Auditing trigger coverage across a database
- The user asks what triggers fire in the database
</usecase>

<examples>
✓ "What triggers are in this database?"
✓ "Does a user-audit trigger exist?" → listTriggers to check
✗ "Show me a trigger's body" → use readQuery against sqlite_schema
</examples>

<what_it_returns>
A sorted JSON array of trigger name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        None
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

impl SqliteHandler {
    /// Lists one page of triggers in the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest { cursor }: ListTriggersRequest,
    ) -> Result<ListTriggersResponse, ErrorData> {
        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT name
            FROM sqlite_schema
            WHERE type = 'trigger' AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (triggers, next_cursor) = pager.finalize(rows);

        Ok(ListTriggersResponse { triggers, next_cursor })
    }
}
