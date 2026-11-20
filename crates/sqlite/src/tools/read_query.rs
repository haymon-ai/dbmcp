//! MCP tool: `readQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::listing::Cursor;
use dbmcp_server::types::{ReadQueryResponse, Truncation, caps, resolve_timeout};

use dbmcp_sql::Dialect;
use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::{Page, plan};
use dbmcp_sql::validation::validate_read;

use super::prelude::*;
use crate::policy::Access;
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
    /// `query_only`, so `SQLite` rejects writes the validator misses. It
    /// then dispatches on the classified [`StatementKind`]: `Select` gets
    /// the page window spliced into its own top-level `LIMIT`/`OFFSET`, or
    /// runs unchanged with the page cut client-side when it cannot be spliced;
    /// `NonSelect` (`EXPLAIN` under the `SQLite` dialect) is executed as-is
    /// and returned in a single page. A malformed `cursor` is rejected by the serde deserializer
    /// before this method is called, which rmcp reports as a tool result
    /// with `isError: true`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if the query is not
    /// read-only, [`SqlError::UseBlocked`] for `USE`,
    /// [`SqlError::DiagnosticsBlocked`] for `SHOW WARNINGS` / `SHOW ERRORS`, or
    /// [`SqlError::Query`] if the backend reports an error. With column
    /// rules, also the policy's rejection or a withheld database error.
    pub async fn read_query(
        &self,
        ReadQueryRequest { query, cursor, timeout }: ReadQueryRequest,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read(&query, Dialect::Sqlite)?;
        let timeout = resolve_timeout(timeout, self.config.query_timeout, self.config.max_query_timeout)?;

        let start = cursor.map_or(0, |c| c.offset);
        let page_size = Some(usize::from(self.config.page_size));
        let (sql, offset, size) = match kind {
            StatementKind::Select => {
                let limit = u64::from(self.config.page_size) + 1;
                match plan(&query, &sqlparser::dialect::SQLiteDialect {}, limit, start) {
                    Page::Merged(sql) => (Cow::Owned(sql), 0, page_size),
                    Page::Skip => (Cow::Borrowed(query.as_str()), start, page_size),
                }
            }
            StatementKind::NonSelect => (Cow::Borrowed(query.as_str()), 0, None),
        };
        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = dbmcp_truncate::Window::new(offset, size, caps);
        let rows = if let Some(policy) = &self.policy {
            self.enforce(policy, &sql, Access::Read, timeout, &mut window)
                .await?
                .rows
        } else {
            self.connection
                .read_json(&*sql, None, timeout, |row| window.push(row))
                .await?;
            self.redactor.redact_rows(window.items).await?
        };
        let fitted = dbmcp_truncate::fit(rows, caps);
        // A paged `SELECT` resumes past its last row; other statements count the rest.
        let (next_cursor, truncation) = match kind {
            StatementKind::Select => {
                // A short page with rows left over was cut by the budget, not by the page size.
                let short = window.rest > 0 && fitted.items.len() < usize::from(self.config.page_size);
                let next_cursor = (fitted.dropped > 0 || window.rest > 0).then(|| Cursor {
                    offset: start.saturating_add(fitted.items.len() as u64),
                });
                (
                    next_cursor,
                    Truncation::new(fitted.cut || fitted.dropped > 0 || short, 0),
                )
            }
            StatementKind::NonSelect => (
                None,
                Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
            ),
        };
        Ok(ReadQueryResponse {
            rows: fitted.items,
            next_cursor,
            truncation,
        })
    }
}
