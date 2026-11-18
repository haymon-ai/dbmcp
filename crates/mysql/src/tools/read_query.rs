//! MCP tool: `readQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::listing::Cursor;
use dbmcp_server::truncate;
use dbmcp_server::types::{ReadQueryRequest, ReadQueryResponse, resolve_timeout};
use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::{Page, plan};
use dbmcp_sql::validation::validate_read;

use super::prelude::*;

const NAME: &str = "readQuery";
const TITLE: &str = "Read Query";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/read_query/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/read_query/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(true)
}

/// Marker type for the `readQuery` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedReadQueryTool;

impl ToolBase for PinnedReadQueryTool {
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
        Some(DESCRIPTION_PINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for PinnedReadQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .read_query(params.query, None, params.cursor, params.timeout)
            .await
    }
}

/// Marker type for the `readQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedReadQueryTool;

impl ToolBase for UnpinnedReadQueryTool {
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
        Some(DESCRIPTION_UNPINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(false))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<MysqlHandler> for UnpinnedReadQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .read_query(params.query, params.database, params.cursor, params.timeout)
            .await
    }
}

impl MysqlHandler {
    /// Executes a read-only SQL query, paginating `SELECT` result rows.
    ///
    /// Validates that the query is read-only and runs it in a read-only
    /// transaction, so the database rejects writes the validator misses.
    /// It then dispatches on the classified [`StatementKind`]: `Select` gets
    /// the page window spliced into its own top-level `LIMIT`/`OFFSET`, or
    /// runs unchanged with the page cut client-side when it cannot be spliced;
    /// `NonSelect` (SHOW, DESCRIBE, EXPLAIN) is executed as-is and
    /// returned in a single page. A malformed `cursor` is rejected by the
    /// serde deserializer before this method is called, which rmcp reports
    /// as a tool result with `isError: true`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if the query is not
    /// read-only, [`SqlError::UseBlocked`] for `USE`,
    /// [`SqlError::DiagnosticsBlocked`] for `SHOW WARNINGS` / `SHOW ERRORS`, or
    /// [`SqlError::Query`] if the backend reports an error.
    pub async fn read_query(
        &self,
        query: String,
        database: Option<String>,
        cursor: Option<Cursor>,
        timeout: Option<u64>,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read(&query, &sqlparser::dialect::MySqlDialect {})?;

        let timeout = resolve_timeout(timeout, self.config.query_timeout, self.config.max_query_timeout)?;
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let start = cursor.map_or(0, |c| c.offset);
        let page_size = Some(usize::from(self.config.page_size));
        let (sql, offset, size) = match kind {
            StatementKind::Select => {
                let limit = u64::from(self.config.page_size) + 1;
                match plan(&query, &sqlparser::dialect::MySqlDialect {}, limit, start) {
                    Page::Merged(sql) => (Cow::Owned(sql), 0, page_size),
                    Page::Skip => (Cow::Borrowed(query.as_str()), start, page_size),
                }
            }
            StatementKind::NonSelect => (Cow::Borrowed(query.as_str()), 0, None),
        };
        let mut window =
            truncate::Window::new(offset, size, self.config.max_cell_chars, self.config.max_response_chars);
        self.connection
            .read_json(&*sql, database, timeout, |row| window.push(row))
            .await?;
        let rows = self.redactor.redact_rows(window.rows).await?;
        let mut fitted = truncate::fit(rows, self.config.max_cell_chars, self.config.max_response_chars);
        // A paged `SELECT` resumes past its last row; other statements count the rest.
        let next_cursor = match kind {
            StatementKind::Select => {
                // A short page with rows left over was cut by the budget, not by the page size.
                if window.rest > 0 && fitted.rows.len() < usize::from(self.config.page_size) {
                    fitted.truncation.truncated = true;
                    fitted.truncation.hint = Some(truncate::HINT);
                }
                (fitted.truncation.rows_omitted.take().is_some() || window.rest > 0).then(|| Cursor {
                    offset: start.saturating_add(fitted.rows.len() as u64),
                })
            }
            StatementKind::NonSelect => {
                fitted.omit(window.rest);
                None
            }
        };
        Ok(ReadQueryResponse {
            rows: fitted.rows,
            next_cursor,
            truncation: fitted.truncation,
        })
    }
}
