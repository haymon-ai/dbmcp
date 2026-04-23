//! MCP tool: `listProcedures`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::{ListProceduresRequest, ListProceduresResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::sanitize::{quote_literal, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `listProcedures` MCP tool.
pub(crate) struct ListProceduresTool;

impl ListProceduresTool {
    const NAME: &'static str = "listProcedures";
    const TITLE: &'static str = "List Procedures";
    const DESCRIPTION: &'static str = r#"List all stored procedures in a specific database.

<usecase>
Use when:
- Exploring a database's stored logic
- Verifying a procedure exists before calling it
- The user asks what procedures are defined
</usecase>

<examples>
✓ "What procedures are in the mydb database?" → listProcedures(database="mydb")
✓ "Does an archive_user procedure exist?" → listProcedures to check
✗ "List functions" → use listFunctions instead
</examples>

<what_it_returns>
A sorted JSON array of procedure name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>"#;
}

impl ToolBase for ListProceduresTool {
    type Parameter = ListProceduresRequest;
    type Output = ListProceduresResponse;
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

impl AsyncTool<MysqlHandler> for ListProceduresTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.list_procedures(params).await
    }
}

impl MysqlHandler {
    /// Lists one page of stored procedures in a database.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorData`] with code `-32602` if `cursor` is malformed,
    /// or an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_procedures(
        &self,
        ListProceduresRequest { database, cursor }: ListProceduresRequest,
    ) -> Result<ListProceduresResponse, ErrorData> {
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
            WHERE ROUTINE_SCHEMA = {} AND ROUTINE_TYPE = 'PROCEDURE'
            ORDER BY ROUTINE_NAME
            LIMIT {} OFFSET {}",
            quote_literal(&database),
            pager.limit(),
            pager.offset(),
        );

        let rows: Vec<String> = self.connection.fetch_scalar(query.as_str(), None).await?;
        let (procedures, next_cursor) = pager.finalize(rows);

        Ok(ListProceduresResponse {
            procedures,
            next_cursor,
        })
    }
}
