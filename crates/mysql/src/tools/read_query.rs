//! MCP tool: `read_query`.

use std::borrow::Cow;

use database_mcp_server::pagination::Pager;
use database_mcp_server::types::{ReadQueryRequest, ReadQueryResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::SqlError;
use database_mcp_sql::StatementKind;
use database_mcp_sql::pagination::with_limit_offset;
use database_mcp_sql::sanitize::validate_ident;
use database_mcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `read_query` MCP tool.
pub(crate) struct ReadQueryTool;

impl ReadQueryTool {
    const NAME: &'static str = "read_query";
    const TITLE: &'static str = "Read Query";
    const DESCRIPTION: &'static str = r#"Execute a read-only SQL query. Allowed statements: SELECT, SHOW, DESCRIBE, USE, EXPLAIN.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Listing server variables or status (SHOW)
- Viewing table structure (DESCRIBE)
- Switching database context (USE)
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use write_query
- Query performance analysis → use explain_query
- Discovering tables or columns → use list_tables or get_table_schema
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✓ "SHOW TABLES" or "DESCRIBE users"
✗ "INSERT INTO users ..." → use write_query
✗ "EXPLAIN SELECT ..." → use explain_query for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.
</what_it_returns>

<pagination>
`SELECT` results are paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. `SHOW`, `DESCRIBE`, `USE`, and `EXPLAIN` return a single page and ignore `cursor`.
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

impl AsyncTool<MysqlHandler> for ReadQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.read_query(params).await?)
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
        ReadQueryRequest {
            query,
            database_name,
            cursor,
        }: ReadQueryRequest,
    ) -> Result<ReadQueryResponse, SqlError> {
        let kind = validate_read_only(&query, &sqlparser::dialect::MySqlDialect {})?;

        let db = Some(database_name.trim()).filter(|s| !s.is_empty());
        if let Some(name) = db {
            validate_ident(name)?;
        }

        match kind {
            StatementKind::Select => {
                let pager = Pager::new(cursor, self.config.page_size);
                let wrapped = with_limit_offset(&query, pager.limit(), pager.offset());
                let rows = self.connection.fetch_json(wrapped.as_str(), db).await?;
                let (rows, next_cursor) = pager.finalize(rows);
                Ok(ReadQueryResponse { rows, next_cursor })
            }
            StatementKind::NonSelect => {
                let rows = self.connection.fetch_json(query.as_str(), db).await?;
                Ok(ReadQueryResponse {
                    rows,
                    next_cursor: None,
                })
            }
        }
    }
}
