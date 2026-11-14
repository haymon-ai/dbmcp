//! MCP tool: `explainQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::truncate;
use dbmcp_server::types::{ExplainQueryRequest, QueryResponse};
use dbmcp_sql::validation::{validate_read, validate_single_statement};

use super::prelude::*;

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

impl AsyncTool<MysqlHandler> for PinnedExplainQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.explain_query(None, params.query, params.analyze).await
    }
}

/// Marker type for the `explainQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedExplainQueryTool;

impl ToolBase for UnpinnedExplainQueryTool {
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

impl AsyncTool<MysqlHandler> for UnpinnedExplainQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .explain_query(params.database, params.query, params.analyze)
            .await
    }
}

impl MysqlHandler {
    /// Returns the execution plan for a query.
    ///
    /// The inner query must be a single statement, and read-only when
    /// `analyze` is true. The plan is always fetched so the database
    /// rejects writes, in every mode.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::MultiStatement`] if the query holds more than
    /// one statement.
    /// Returns [`SqlError::ReadOnlyViolation`] if `analyze` is true and
    /// the query is a write statement.
    /// Returns [`SqlError::Query`] if the backend reports an error, including
    /// a plain `EXPLAIN` of a write statement.
    pub async fn explain_query(
        &self,
        database: Option<String>,
        query: String,
        analyze: bool,
    ) -> Result<QueryResponse, ErrorData> {
        if analyze {
            let _ = validate_read(&query, &sqlparser::dialect::MySqlDialect {})?;
        } else {
            validate_single_statement(&query, &sqlparser::dialect::MySqlDialect {})?;
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = if analyze {
            format!("EXPLAIN ANALYZE {query}")
        } else {
            format!("EXPLAIN FORMAT=JSON {query}")
        };

        // ANALYZE executes the statement, and planning alone can run user functions.
        let rows = self
            .connection
            .read_json(explain_sql.as_str(), database, self.config.query_timeout)
            .await?;
        let rows = self.redactor.redact_rows(rows).await?;
        let fitted = truncate::fit(rows, self.config.max_cell_chars, self.config.max_response_chars);

        Ok(QueryResponse {
            rows: fitted.rows,
            truncation: fitted.truncation,
        })
    }
}
