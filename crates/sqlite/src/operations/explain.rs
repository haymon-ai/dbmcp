//! Logic of the `explain` tool.

use dbmcp_policy::Access;
use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::ExplainResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::validation::validate_single_statement;
use dbmcp_truncate::{Window, fit};
use rmcp::model::ErrorData;

use crate::SqliteHandler;
use crate::queries;
use crate::types::ExplainRequest;

impl SqliteHandler {
    /// Returns the execution plan for a query.
    ///
    /// Always uses `EXPLAIN QUERY PLAN` — `SQLite` does not support
    /// `EXPLAIN ANALYZE`. With column rules, the plan of a statement
    /// touching a ruled relation is pruned to value-free keys.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::MultiStatement`] if the query holds more than
    /// one statement.
    /// Returns [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::MultiStatement`]: dbmcp_sql::SqlError::MultiStatement
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    pub async fn explain(&self, ExplainRequest { query }: ExplainRequest) -> Result<ExplainResponse, ErrorData> {
        validate_single_statement(&query, &sqlparser::dialect::SQLiteDialect {})?;

        let explain_sql = queries::explain::statement(&query);

        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(0, None, caps);
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
            self.redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?
        };
        let fitted = fit(rows, caps);

        Ok(ExplainResponse {
            rows: fitted.items,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
