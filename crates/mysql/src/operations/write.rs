//! Logic of the `write` tool.

use dbmcp_policy::Access;
use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::WriteResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::validation::validate_write;
use dbmcp_truncate::{Window, fit};
use rmcp::model::ErrorData;

use crate::MysqlHandler;

impl MysqlHandler {
    /// Executes a write SQL query.
    ///
    /// Rejects `USE`: it would change the database of the pooled connection
    /// for later queries.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::UnreadableCharacter`] for SQL the server reads
    /// otherwise, [`SqlError::UseBlocked`] for `USE`,
    /// [`SqlError::ReadOnlyViolation`] in read-only mode, or [`SqlError`]
    /// if the query fails. With column rules, also
    /// [`SqlError::MultiStatement`], [`SqlError::ExecutableCommentBlocked`],
    /// the policy's rejection, or a withheld database error.
    ///
    /// [`SqlError`]: dbmcp_sql::SqlError
    /// [`SqlError::ExecutableCommentBlocked`]: dbmcp_sql::SqlError::ExecutableCommentBlocked
    /// [`SqlError::MultiStatement`]: dbmcp_sql::SqlError::MultiStatement
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    /// [`SqlError::UnreadableCharacter`]: dbmcp_sql::SqlError::UnreadableCharacter
    /// [`SqlError::UseBlocked`]: dbmcp_sql::SqlError::UseBlocked
    pub async fn write(&self, query: String, database: Option<String>) -> Result<WriteResponse, ErrorData> {
        validate_write(&query, &sqlparser::dialect::MySqlDialect {})?;

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(0, None, caps);
        let (rows, rows_affected, last_insert_id) = if let Some(policy) = &self.policy {
            let timeout = self.connection.query_timeout();
            let enforced = self
                .enforce(policy, &query, Access::Write, timeout, database, &mut window)
                .await?;
            (enforced.rows, enforced.rows_affected, enforced.last_insert_id)
        } else {
            let (rows_affected, last_insert_id) = self
                .connection
                .execute_json(query.as_str(), database, |row| window.push(row))
                .await?;
            let rows = self
                .redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            (rows, rows_affected, last_insert_id)
        };
        let fitted = fit(rows, caps);

        Ok(WriteResponse {
            rows: fitted.items,
            rows_affected,
            last_insert_id,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
