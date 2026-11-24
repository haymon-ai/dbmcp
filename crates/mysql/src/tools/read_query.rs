//! MCP tool: `readQuery`.

use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::listing::Cursor;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::{ReadQueryRequest, ReadQueryResponse, resolve_timeout};
use dbmcp_sql::Dialect;
use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::{Page, plan};
use dbmcp_sql::validation::validate_read;
use dbmcp_truncate::{Window, fit};
use dbmcp_types::database;

use super::prelude::*;
use dbmcp_policy::Access;

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

/// Marker type for the `readQuery` MCP tool.
pub(crate) struct ReadQueryTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ReadQueryTool<PINNED> {
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
        Some(
            if PINNED {
                DESCRIPTION_PINNED
            } else {
                DESCRIPTION_UNPINNED
            }
            .into(),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl<const PINNED: bool> AsyncTool<MysqlHandler> for ReadQueryTool<PINNED> {
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
    /// [`SqlError::DiagnosticsBlocked`] for `SHOW WARNINGS` / `SHOW ERRORS`,
    /// [`SqlError::ExecutableCommentBlocked`] for `/*! … */` / `/*M! … */`,
    /// [`SqlError::UnreadableCharacter`] for SQL the server reads otherwise, or
    /// [`SqlError::Query`] if the backend reports an error. With column
    /// rules, also the policy's rejection or a withheld database error.
    ///
    /// [`SqlError::DiagnosticsBlocked`]: dbmcp_sql::SqlError::DiagnosticsBlocked
    /// [`SqlError::ExecutableCommentBlocked`]: dbmcp_sql::SqlError::ExecutableCommentBlocked
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    /// [`SqlError::UnreadableCharacter`]: dbmcp_sql::SqlError::UnreadableCharacter
    /// [`SqlError::UseBlocked`]: dbmcp_sql::SqlError::UseBlocked
    pub async fn read_query(
        &self,
        query: String,
        database: Option<String>,
        cursor: Option<Cursor>,
        timeout: Option<u64>,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read(
            &query,
            match self.config.backend {
                database::Backend::Mariadb => Dialect::Mariadb,
                _ => Dialect::Mysql,
            },
        )?;

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
        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(offset, size, caps);
        let rows = if let Some(policy) = &self.policy {
            self.enforce(policy, &sql, Access::Read, timeout, database, &mut window)
                .await?
                .rows
        } else {
            self.connection
                .read_json(&*sql, database, timeout, |row| window.push(row))
                .await?;
            self.redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?
        };
        let fitted = fit(rows, caps);
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
