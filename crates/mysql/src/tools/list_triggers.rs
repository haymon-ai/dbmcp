//! MCP tool: `listTriggers`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ListTriggersRequest, ListTriggersResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::{quote_literal, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ListTriggersTool {
    const NAME: &'static str = "listTriggers";
    const TITLE: &'static str = "List Triggers";
    const DESCRIPTION: &'static str = r#"List all triggers in a specific database. Requires `database` — call `listDatabases` first to discover available databases.

<usecase>
Use when:
- Investigating side-effects on INSERT/UPDATE/DELETE for a table
- Auditing trigger coverage across a database
- The user asks what triggers fire in a database
</usecase>

<examples>
✓ "What triggers are in the mydb database?" → listTriggers(database="mydb")
✓ "Does an order-audit trigger exist?" → listTriggers to check
✗ "Show me a trigger's body" → use readQuery against information_schema.TRIGGERS
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

impl MysqlHandler {
    /// Lists one page of triggers in a database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest { database, cursor }: ListTriggersRequest,
    ) -> Result<ListTriggersResponse, ErrorData> {
        validate_ident(&database)?;

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT CAST(TRIGGER_NAME AS CHAR)
            FROM information_schema.TRIGGERS
            WHERE TRIGGER_SCHEMA = {}
            ORDER BY TRIGGER_NAME
            LIMIT {} OFFSET {}",
            quote_literal(&database),
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (triggers, next_cursor) = pager.finalize(rows);

        Ok(ListTriggersResponse { triggers, next_cursor })
    }
}
