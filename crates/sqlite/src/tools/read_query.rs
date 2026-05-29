//! MCP tool: `readQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::pagination::Pager;
use dbmcp_server::types::ReadQueryResponse;

use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::with_limit_offset;
use dbmcp_sql::validation::validate_read_only;

use super::prelude::*;
use crate::types::ReadQueryRequest;

const NAME: &str = "readQuery";
const TITLE: &str = "Read Query";
const DESCRIPTION: &str = include_str!("../../assets/tools/read_query.md");

/// Marker type for the `readQuery` MCP tool.
pub(crate) struct ReadQueryTool;

impl ToolBase for ReadQueryTool {
    type Parameter = ReadQueryRequest;
    type Output = ReadQueryResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<SqliteHandler> for ReadQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.read_query(params).await
    }
}

impl SqliteHandler {
    /// Executes a read-only SQL query, paginating `SELECT` result rows.
    ///
    /// Validates that the query is read-only, then dispatches on the
    /// classified [`StatementKind`]: `Select` is wrapped in a subquery with
    /// a server-controlled `LIMIT`/`OFFSET`; `NonSelect` (`EXPLAIN` under
    /// the `SQLite` dialect) is executed as-is and returned in a single
    /// page. A malformed `cursor` is rejected by the serde deserializer
    /// before this method is called, producing JSON-RPC `-32602`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`SqlError::Query`] if the backend reports an error.
    pub async fn read_query(
        &self,
        ReadQueryRequest { query, cursor }: ReadQueryRequest,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read_only(&query, &sqlparser::dialect::SQLiteDialect {})?;

        let (rows, next_cursor) = match kind {
            StatementKind::Select => {
                let pager = Pager::new(cursor, self.config.page_size);
                let wrapped = with_limit_offset(&query, pager.limit(), pager.offset());
                let rows = self.connection.fetch_json(wrapped.as_str(), None).await?;
                pager.paginate(rows)
            }
            StatementKind::NonSelect => {
                let rows = self.connection.fetch_json(query.as_str(), None).await?;
                (rows, None)
            }
        };
        let rows = self.redactor.redact_rows(rows).await?;
        Ok(ReadQueryResponse { rows, next_cursor })
    }
}
