//! MCP tool: `readQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::listing;
use dbmcp_server::truncate;
use dbmcp_server::types::{ReadQueryResponse, resolve_timeout};

use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::with_limit_offset;
use dbmcp_sql::validation::validate_read;

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
    /// Validates that the query is read-only and runs it under
    /// `query_only`, so `SQLite` rejects writes the validator misses.
    /// It then dispatches on the
    /// classified [`StatementKind`]: `Select` is wrapped in a subquery with
    /// a server-controlled `LIMIT`/`OFFSET`; `NonSelect` (`EXPLAIN` under
    /// the `SQLite` dialect) is executed as-is and returned in a single
    /// page. A malformed `cursor` is rejected by the serde deserializer
    /// before this method is called, which rmcp reports as a tool result
    /// with `isError: true`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if the query is not
    /// read-only, [`SqlError::UseBlocked`] for `USE`, or [`SqlError::Query`]
    /// if the backend reports an error.
    pub async fn read_query(
        &self,
        ReadQueryRequest { query, cursor, timeout }: ReadQueryRequest,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read(&query, &sqlparser::dialect::SQLiteDialect {})?;
        let timeout = resolve_timeout(timeout, self.config.query_timeout, self.config.max_query_timeout)?;

        let (rows, next_cursor) = match kind {
            StatementKind::Select => {
                listing::paginate(cursor, self.config.page_size, async |page| {
                    let wrapped = with_limit_offset(&query, page.limit(), page.offset());
                    self.connection.read_json(wrapped.as_str(), None, timeout).await
                })
                .await?
            }
            StatementKind::NonSelect => {
                let rows = self.connection.read_json(query.as_str(), None, timeout).await?;
                (rows, None)
            }
        };
        let rows = self.redactor.redact_rows(rows).await?;
        let mut fitted = truncate::fit(rows, self.config.max_cell_chars, self.config.max_response_chars);
        // A paged `SELECT` resumes dropped rows; other statements count them.
        let next_cursor = match kind {
            StatementKind::Select => fitted.resume(cursor, next_cursor),
            StatementKind::NonSelect => next_cursor,
        };
        Ok(ReadQueryResponse {
            rows: fitted.rows,
            next_cursor,
            truncation: fitted.truncation,
        })
    }
}
