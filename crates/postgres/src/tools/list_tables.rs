//! MCP tool: `listTables`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ListTablesRequest, ListTablesResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listTables` MCP tool.
pub(crate) struct ListTablesTool;

impl ListTablesTool {
    const NAME: &'static str = "listTables";
    const TITLE: &'static str = "List Tables";
    const DESCRIPTION: &'static str = r#"List all tables in a specific database. Requires `database` — call `listDatabases` first to discover available databases.

<usecase>
Use when:
- Exploring a database to find relevant tables
- Verifying a table exists before querying or inspecting it
- The user asks what tables are in a database
</usecase>

<examples>
✓ "What tables are in the mydb database?" → listTables(database="mydb")
✓ "Does a users table exist?" → listTables to check
✗ "Show me the columns of users" → use getTableSchema instead
</examples>

<what_it_returns>
A sorted JSON array of table name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>"#;
}

impl ToolBase for ListTablesTool {
    type Parameter = ListTablesRequest;
    type Output = ListTablesResponse;
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

impl AsyncTool<PostgresHandler> for ListTablesTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_tables(params).await
    }
}

impl PostgresHandler {
    /// Lists one page of tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_tables(
        &self,
        ListTablesRequest { database, cursor }: ListTablesRequest,
    ) -> Result<ListTablesResponse, ErrorData> {
        let db = Some(database.trim()).filter(|s| !s.is_empty());
        if let Some(name) = db {
            validate_ident(name)?;
        }

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT tablename
            FROM pg_tables
            WHERE schemaname = 'public'
            ORDER BY tablename
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), db).await?;
        let (tables, next_cursor) = pager.finalize(rows);

        Ok(ListTablesResponse { tables, next_cursor })
    }
}
