//! MCP tool: `readQuery`.

use std::borrow::Cow;

use dbmcp_server::pagination::{Cursor, Pager};
use dbmcp_server::types::{PinnedReadQueryRequest, ReadQueryResponse, UnpinnedReadQueryRequest};
use dbmcp_sql::Connection as _;
use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::with_limit_offset;
use dbmcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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
    type Parameter = PinnedReadQueryRequest;
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
}

impl AsyncTool<MysqlHandler> for PinnedReadQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.read_query(params.query, None, params.cursor).await
    }
}

/// Marker type for the `readQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedReadQueryTool;

impl ToolBase for UnpinnedReadQueryTool {
    type Parameter = UnpinnedReadQueryRequest;
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
}

impl AsyncTool<MysqlHandler> for UnpinnedReadQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .read_query(params.inner.query, params.database, params.inner.cursor)
            .await
    }
}

impl MysqlHandler {
    /// Executes a read-only SQL query, paginating `SELECT` result rows.
    ///
    /// Validates that the query is read-only, then dispatches on the
    /// classified [`StatementKind`]: `Select` is wrapped in a subquery with
    /// a server-controlled `LIMIT`/`OFFSET`; `NonSelect` (SHOW, DESCRIBE, USE,
    /// EXPLAIN) is executed as-is and returned in a single page. A malformed
    /// `cursor` is rejected by the serde deserializer before this method is
    /// called, producing JSON-RPC `-32602`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`SqlError::Query`] if the backend reports an error.
    pub async fn read_query(
        &self,
        query: String,
        database: Option<String>,
        cursor: Option<Cursor>,
    ) -> Result<ReadQueryResponse, ErrorData> {
        let kind = validate_read_only(&query, &sqlparser::dialect::MySqlDialect {})?;

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        match kind {
            StatementKind::Select => {
                let pager = Pager::new(cursor, self.config.page_size);
                let wrapped = with_limit_offset(&query, pager.limit(), pager.offset());
                let rows = self.connection.fetch_json(wrapped.as_str(), database).await?;
                let (mut rows, next_cursor) = pager.paginate(rows);
                if let Some(r) = &self.redactor {
                    r.apply(&mut rows)?;
                }
                Ok(ReadQueryResponse { rows, next_cursor })
            }
            StatementKind::NonSelect => {
                let mut rows = self.connection.fetch_json(query.as_str(), database).await?;
                if let Some(r) = &self.redactor {
                    r.apply(&mut rows)?;
                }
                Ok(ReadQueryResponse {
                    rows,
                    next_cursor: None,
                })
            }
        }
    }
}
