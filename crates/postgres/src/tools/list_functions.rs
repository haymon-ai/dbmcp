//! MCP tool: `listFunctions`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ListFunctionsRequest, ListFunctionsResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::validate_ident;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `listFunctions` MCP tool.
pub(crate) struct ListFunctionsTool;

impl ListFunctionsTool {
    const NAME: &'static str = "listFunctions";
    const TITLE: &'static str = "List Functions";
    const DESCRIPTION: &'static str = r#"List all user-defined functions in the `public` schema of a database. Aggregates, window functions, and procedures are excluded.

<usecase>
Use when:
- Exploring a database's stored logic
- Verifying a function exists before calling it
- The user asks what functions are defined
</usecase>

<examples>
✓ "What functions are in the mydb database?" → listFunctions(database="mydb")
✓ "Does a calc_total function exist?" → listFunctions to check
✗ "List stored procedures" → use listProcedures instead
</examples>

<what_it_returns>
A sorted JSON array of function name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>"#;
}

impl ToolBase for ListFunctionsTool {
    type Parameter = ListFunctionsRequest;
    type Output = ListFunctionsResponse;
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

impl AsyncTool<PostgresHandler> for ListFunctionsTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_functions(params).await
    }
}

impl PostgresHandler {
    /// Lists one page of user-defined functions in the `public` schema.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_functions(
        &self,
        ListFunctionsRequest { database, cursor }: ListFunctionsRequest,
    ) -> Result<ListFunctionsResponse, ErrorData> {
        let db = Some(database.trim()).filter(|s| !s.is_empty());
        if let Some(name) = db {
            validate_ident(name)?;
        }

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT p.proname
            FROM pg_proc p
            JOIN pg_namespace n ON p.pronamespace = n.oid
            WHERE n.nspname = 'public' AND p.prokind = 'f'
            ORDER BY p.proname
            LIMIT {} OFFSET {}",
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), db).await?;
        let (functions, next_cursor) = pager.finalize(rows);

        Ok(ListFunctionsResponse { functions, next_cursor })
    }
}
