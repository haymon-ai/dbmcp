//! MCP tool: `listTriggers`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListTriggersRequest, ListTriggersResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listTriggers` MCP tool.
pub(crate) struct ListTriggersTool;

impl ListTriggersTool {
    const NAME: &'static str = "listTriggers";
    const TITLE: &'static str = "List Triggers";
    const DESCRIPTION: &'static str = r#"List all user-defined triggers on tables in the `public` schema of a database. Internal constraint and foreign-key triggers are excluded.

<usecase>
Use when:
- Investigating side-effects on INSERT/UPDATE/DELETE for a table
- Auditing trigger coverage across a database
- The user asks what triggers fire in a database
</usecase>

<examples>
✓ "What triggers are in the mydb database?" → listTriggers(database="mydb")
✓ "Does an order-audit trigger exist?" → listTriggers to check
✗ "Show me a trigger's body" → use readQuery against pg_trigger or information_schema.triggers
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

impl AsyncTool<PostgresHandler> for ListTriggersTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_triggers(params).await
    }
}

impl PostgresHandler {
    /// Lists one page of user-defined triggers on tables in the `public` schema.
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
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(validate_ident)
            .transpose()?;

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT t.tgname
            FROM pg_trigger t
            JOIN pg_class c ON t.tgrelid = c.oid
            JOIN pg_namespace n ON c.relnamespace = n.oid
            WHERE n.nspname = 'public' AND NOT t.tgisinternal
            ORDER BY t.tgname
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), database).await?;
        let (triggers, next_cursor) = pager.finalize(rows);

        Ok(ListTriggersResponse { triggers, next_cursor })
    }
}
