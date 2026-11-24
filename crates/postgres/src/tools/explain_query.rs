//! MCP tool: `explainQuery`.

use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::{ExplainQueryRequest, QueryResponse, resolve_timeout};
use dbmcp_sql::Dialect;
use dbmcp_sql::validation::{validate_read, validate_single_statement};
use dbmcp_truncate::{Window, fit};

use super::prelude::*;
use dbmcp_policy::Access;

const NAME: &str = "explainQuery";
const TITLE: &str = "Explain Query";
const DESCRIPTION: &str = include_str!("../../assets/tools/explain_query.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(true)
}

/// Marker type for the `explainQuery` MCP tool.
///
/// `PINNED` serves a database pinned by configuration: the input schema has
/// no `database` field, and the pinned route drops one sent anyway.
pub(crate) struct ExplainQueryTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ExplainQueryTool<PINNED> {
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
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl<const PINNED: bool> AsyncTool<PostgresHandler> for ExplainQueryTool<PINNED> {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .explain_query(params.database, params.query, params.analyze, params.timeout)
            .await
    }
}

impl PostgresHandler {
    /// Returns the execution plan for a query.
    ///
    /// The inner query must be a single statement, and read-only when
    /// `analyze` is true. The plan is always fetched so the database
    /// rejects writes, in every mode. With column rules, the plan of a
    /// statement touching a ruled relation is pruned to value-free keys,
    /// and `analyze` on one is rejected.
    ///
    /// # Errors
    ///
    /// Returns an invalid-params error if [`resolve_timeout`] rejects `timeout`.
    /// Returns [`SqlError::MultiStatement`] if the query holds more than
    /// one statement.
    /// Returns [`SqlError::ReadOnlyViolation`] if `analyze` is true and
    /// the query is a write statement.
    /// Returns [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::MultiStatement`]: dbmcp_sql::SqlError::MultiStatement
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn explain_query(
        &self,
        database: Option<String>,
        query: String,
        analyze: bool,
        timeout: Option<u64>,
    ) -> Result<QueryResponse, ErrorData> {
        if analyze {
            let _ = validate_read(&query, Dialect::Postgres)?;
        } else {
            validate_single_statement(&query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let timeout = resolve_timeout(timeout, self.config.query_timeout, self.config.max_query_timeout)?;
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = if analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}")
        } else {
            format!("EXPLAIN (FORMAT JSON) {query}")
        };

        // ANALYZE executes the statement, and planning alone can run user functions.
        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(0, None, caps);
        let rows = if let Some(policy) = &self.policy {
            self.enforce(policy, &explain_sql, Access::Read, timeout, database, &mut window)
                .await?
                .rows
        } else {
            self.connection
                .read_json(explain_sql.as_str(), database, timeout, |row| {
                    window.push(row);
                })
                .await?;
            self.redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?
        };
        let fitted = fit(rows, caps);

        Ok(QueryResponse {
            rows: fitted.items,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
