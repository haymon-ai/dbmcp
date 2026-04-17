//! SQL-layer error types for validation, timeout, and query failures.

/// Errors produced by SQL validation, identifier checking, and query execution.
#[derive(Debug, thiserror::Error)]
pub enum SqlError {
    /// Query blocked by read-only mode.
    #[error("Query blocked: only SELECT, SHOW, DESC, DESCRIBE, USE queries are allowed in read-only mode")]
    ReadOnlyViolation,

    /// `LOAD_FILE()` function blocked for security.
    #[error("Operation forbidden: LOAD_FILE() is not allowed for security reasons")]
    LoadFileBlocked,

    /// INTO OUTFILE/DUMPFILE blocked for security.
    #[error("Operation forbidden: SELECT INTO OUTFILE/DUMPFILE is not allowed for security reasons")]
    IntoOutfileBlocked,

    /// Multiple SQL statements blocked.
    #[error("Query blocked: only single statements are allowed")]
    MultiStatement,

    /// Invalid database or table name identifier.
    #[error("Invalid identifier '{0}': must not be empty, whitespace-only, or contain control characters")]
    InvalidIdentifier(String),

    /// Query exceeded the configured timeout.
    #[error("Query timed out after {elapsed_secs:.1}s: {sql}")]
    QueryTimeout {
        /// Wall-clock seconds elapsed before cancellation.
        elapsed_secs: f64,
        /// The SQL statement that was cancelled.
        sql: String,
    },

    /// Database query execution failed.
    #[error("Database error: {0}")]
    Query(String),

    /// Table not found in database.
    #[error("Table not found: {0}")]
    TableNotFound(String),
}

impl From<SqlError> for rmcp::model::ErrorData {
    fn from(e: SqlError) -> Self {
        Self::internal_error(e.to_string(), None)
    }
}
