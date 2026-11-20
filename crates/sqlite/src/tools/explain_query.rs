//! MCP tool: `explainQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::{QueryResponse, Truncation, caps};
use dbmcp_sql::validation::validate_single_statement;

use super::prelude::*;
use crate::policy::Access;
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
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
    /// `EXPLAIN ANALYZE`. With column rules, explaining a statement that
    /// touches a ruled relation is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::MultiStatement`] if the query holds more than
    /// one statement.
    /// Returns [`SqlError::Query`] if the backend reports an error.
    pub async fn explain_query(
        &self,
        ExplainQueryRequest { query }: ExplainQueryRequest,
    ) -> Result<QueryResponse, ErrorData> {
        validate_single_statement(&query, &sqlparser::dialect::SQLiteDialect {})?;

        let explain_sql = format!("EXPLAIN QUERY PLAN {query}");

        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = dbmcp_truncate::Window::new(0, None, caps);
        let rows = if let Some(policy) = &self.policy {
            self.enforce(
                policy,
                &explain_sql,
                Access::Read,
                self.config.query_timeout,
                &mut window,
            )
            .await?
            .rows
        } else {
            self.connection
                .read_json(explain_sql.as_str(), None, self.config.query_timeout, |row| {
                    window.push(row);
                })
                .await?;
            self.redactor.redact_rows(window.items).await?
        };
        let fitted = dbmcp_truncate::fit(rows, caps);

        Ok(QueryResponse {
            rows: fitted.items,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
