//! MCP tool: `listFunctions`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListFunctionsRequest, ListFunctionsResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::{quote_literal, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `listFunctions` MCP tool.
pub(crate) struct ListFunctionsTool;

impl ListFunctionsTool {
    const NAME: &'static str = "listFunctions";
    const TITLE: &'static str = "List Functions";
    const DESCRIPTION: &'static str = r#"List all stored SQL functions in a specific database. Loadable UDFs (`mysql.func`) are not included.

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

impl AsyncTool<MysqlHandler> for ListFunctionsTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_functions(params).await
    }
}

impl MysqlHandler {
    /// Lists one page of stored functions in a database.
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
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(|| self.connection.default_database_name().to_owned(), str::to_owned);

        validate_ident(&database)?;

        let pager = Pager::new(cursor, self.config.page_size);
        let query = format!(
            r"
            SELECT CAST(ROUTINE_NAME AS CHAR)
            FROM information_schema.ROUTINES
            WHERE ROUTINE_SCHEMA = {} AND ROUTINE_TYPE = 'FUNCTION'
            ORDER BY ROUTINE_NAME
            LIMIT {} OFFSET {}",
            quote_literal(&database),
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (functions, next_cursor) = pager.finalize(rows);

        Ok(ListFunctionsResponse { functions, next_cursor })
    }
}
