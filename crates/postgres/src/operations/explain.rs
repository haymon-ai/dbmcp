//! Logic of the `explain` tool.

use dbmcp_policy::Access;
use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::{ExplainResponse, resolve_timeout};
use dbmcp_sql::Connection as _;
use dbmcp_sql::Dialect;
use dbmcp_sql::validation::{validate_read, validate_single_statement};
use dbmcp_truncate::{Window, fit};
use rmcp::model::ErrorData;

use crate::PostgresHandler;
use crate::queries;

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
    pub async fn explain(
        &self,
        database: Option<String>,
        query: String,
        analyze: bool,
        timeout: Option<u64>,
    ) -> Result<ExplainResponse, ErrorData> {
        if analyze {
            let _ = validate_read(&query, Dialect::Postgres)?;
        } else {
            validate_single_statement(&query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let timeout = resolve_timeout(timeout, self.config.query_timeout, self.config.max_query_timeout)?;
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let explain_sql = queries::explain::statement(&query, analyze);

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

        Ok(ExplainResponse {
            rows: fitted.items,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
