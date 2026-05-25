//! MCP tool: `explainQuery`.

use std::borrow::Cow;

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::{PinnedExplainQueryRequest, QueryResponse, UnpinnedExplainQueryRequest};
use dbmcp_sql::Connection as _;
use dbmcp_sql::validation::validate_read_only;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

const NAME: &str = "explainQuery";
const TITLE: &str = "Explain Query";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/explain_query/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/explain_query/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(true)
}

/// Marker type for the `explainQuery` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedExplainQueryTool;

impl ToolBase for PinnedExplainQueryTool {
    type Parameter = PinnedExplainQueryRequest;
    type Output = QueryResponse;
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

impl AsyncTool<PostgresHandler> for PinnedExplainQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.explain_query(None, params.query, params.analyze).await
    }
}

/// Marker type for the `explainQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedExplainQueryTool;

impl ToolBase for UnpinnedExplainQueryTool {
    type Parameter = UnpinnedExplainQueryRequest;
    type Output = QueryResponse;
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

impl AsyncTool<PostgresHandler> for UnpinnedExplainQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .explain_query(params.database, params.inner.query, params.inner.analyze)
            .await
    }
}

impl PostgresHandler {
    /// Returns the execution plan for a query.
    ///
    /// When `analyze` is true and read-only mode is enabled, the inner
    /// query is validated to be read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] if `analyze` is true,
    /// read-only mode is enabled, and the query is a write statement.
    /// Returns [`SqlError::Query`] if the backend reports an error.
    pub async fn explain_query(
        &self,
        database: Option<String>,
        query: String,
        analyze: bool,
    ) -> Result<QueryResponse, ErrorData> {
        if analyze && self.config.read_only {
            let _ = validate_read_only(&query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = if analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}")
        } else {
            format!("EXPLAIN (FORMAT JSON) {query}")
        };

        let rows = self.connection.fetch_json(explain_sql.as_str(), database).await?;
        let rows = self.redactor.redact_rows(rows).await?;

        Ok(QueryResponse { rows })
    }
}
