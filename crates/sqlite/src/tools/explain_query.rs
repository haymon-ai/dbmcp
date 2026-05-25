//! MCP tool: `explainQuery`.

use std::borrow::Cow;

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::QueryResponse;

use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ExplainQueryRequest;

const NAME: &str = "explainQuery";
const TITLE: &str = "Explain Query";
const DESCRIPTION: &str = include_str!("../../assets/tools/explain_query.md");

/// Marker type for the `explainQuery` MCP tool.
pub(crate) struct ExplainQueryTool;

impl ToolBase for ExplainQueryTool {
    type Parameter = ExplainQueryRequest;
    type Output = QueryResponse;
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
}

impl AsyncTool<SqliteHandler> for ExplainQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.explain_query(params).await
    }
}

impl SqliteHandler {
    /// Returns the execution plan for a query.
    ///
    /// Always uses `EXPLAIN QUERY PLAN` — `SQLite` does not support
    /// `EXPLAIN ANALYZE`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::Query`] if the backend reports an error.
    pub async fn explain_query(
        &self,
        ExplainQueryRequest { query }: ExplainQueryRequest,
    ) -> Result<QueryResponse, ErrorData> {
        let explain_sql = format!("EXPLAIN QUERY PLAN {query}");

        let rows = self.connection.fetch_json(explain_sql.as_str(), None).await?;
        let rows = self.redactor.redact_rows(rows).await?;

        Ok(QueryResponse { rows })
    }
}
