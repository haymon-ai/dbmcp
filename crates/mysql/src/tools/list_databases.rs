//! MCP tool: `list_databases`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ListDatabasesRequest, ListDatabasesResponse};
use database_mcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `list_databases` MCP tool.
pub(crate) struct ListDatabasesTool;

impl ListDatabasesTool {
    const NAME: &'static str = "list_databases";
    const TITLE: &'static str = "List Databases";
    const DESCRIPTION: &'static str = r#"List all accessible databases on the connected server. Use this tool to discover what databases are available before using other tools.

<usecase>
ALWAYS call this tool FIRST when:
- You need to explore what databases exist on the server
- You need a database name for list_tables, get_table_schema, or query tools
- The user asks what data is available
</usecase>

<examples>
✓ "What databases are on this server?"
✓ "Show me what's available" → call list_databases first
</examples>

<what_it_returns>
A sorted JSON array of database name strings.
</what_it_returns>

<pagination>
This tool paginates its response. If more databases exist than fit in one page, the response includes a `nextCursor` string — call `list_databases` again with that string as the `cursor` argument to fetch the next page. Iterate until `nextCursor` is absent.

Cursors are opaque: do not parse, modify, or persist them across sessions. Passing a malformed or stale cursor returns a JSON-RPC error (code -32602); recover by retrying without a cursor to restart from the first page.

Note: databases created or dropped between paginated calls may cause the same database to appear twice or to be skipped. Re-enumerate from a fresh call for a consistent snapshot.
</pagination>"#;
}

impl ToolBase for ListDatabasesTool {
    type Parameter = ListDatabasesRequest;
    type Output = ListDatabasesResponse;
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

impl AsyncTool<MysqlHandler> for ListDatabasesTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_databases(&params).await
    }
}

impl MysqlHandler {
    /// Lists one page of accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `request.cursor` is
    /// malformed, or an internal-error [`ErrorData`] if the underlying
    /// query fails.
    pub async fn list_databases(&self, request: &ListDatabasesRequest) -> Result<ListDatabasesResponse, ErrorData> {
        let pager = Pager::new(request.cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT CAST(SCHEMA_NAME AS CHAR)
            FROM information_schema.SCHEMATA
            ORDER BY SCHEMA_NAME
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (databases, next_cursor) = pager.finalize(rows);

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::ListDatabasesTool;

    #[test]
    fn description_documents_pagination() {
        let desc = ListDatabasesTool::DESCRIPTION;
        assert!(desc.contains("nextCursor"), "description must mention `nextCursor`");
        assert!(desc.contains("cursor"), "description must document cursor semantics");
        assert!(
            desc.contains("-32602"),
            "description must mention the invalid-cursor error code"
        );
    }

    #[test]
    fn description_does_not_state_specific_page_size() {
        assert!(
            !ListDatabasesTool::DESCRIPTION.contains("100"),
            "description must not hard-state `100` databases — page size is operator-configurable"
        );
    }
}
