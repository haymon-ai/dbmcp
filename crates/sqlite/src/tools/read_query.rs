//! MCP tool: `readQuery`.

use std::borrow::Cow;

use dbmcp_server::pagination::Pager;
use dbmcp_server::types::ReadQueryResponse;

use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use dbmcp_sql::StatementKind;
use dbmcp_sql::pagination::with_limit_offset;
use dbmcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ReadQueryRequest;

/// Marker type for the `readQuery` MCP tool.
pub(crate) struct ReadQueryTool;

impl ReadQueryTool {
    const NAME: &'static str = "readQuery";
    const TITLE: &'static str = "Read Query";
    const DESCRIPTION: &'static str = r#"Execute a read-only SQL query. Allowed statements: SELECT, EXPLAIN.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Checking data existence or counts
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use writeQuery
- Query performance analysis → use explainQuery
- Discovering tables or columns → use listTables or getTableSchema
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✗ "INSERT INTO users ..." → use writeQuery
✗ "EXPLAIN SELECT ..." → use explainQuery for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.
</what_it_returns>

<pagination>
`SELECT` results are paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. `EXPLAIN` returns a single page and ignores `cursor`.
</pagination>"#;
}

impl ToolBase for ReadQueryTool {
    type Parameter = ReadQueryRequest;
    type Output = ReadQueryResponse;
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
                .open_world(true),
        )
    }
}

impl AsyncTool<SqliteHandler> for ReadQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.read_query(params).await?)
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
    ) -> Result<ReadQueryResponse, SqlError> {
        let kind = validate_read_only(&query, &sqlparser::dialect::SQLiteDialect {})?;

        match kind {
            StatementKind::Select => {
                let pager = Pager::new(cursor, self.config.page_size);
                let wrapped = with_limit_offset(&query, pager.limit(), pager.offset());
                let rows = self.connection.fetch_json(wrapped.as_str(), None).await?;
                let (rows, next_cursor) = pager.finalize(rows);
                Ok(ReadQueryResponse { rows, next_cursor })
            }
            StatementKind::NonSelect => {
                let rows = self.connection.fetch_json(query.as_str(), None).await?;
                Ok(ReadQueryResponse {
                    rows,
                    next_cursor: None,
                })
            }
        }
    }
}
