//! Application error types for the MCP server.
//!
//! Defines [`AppError`] with variants for connection, security validation,
//! and query execution failures. Configuration errors live in the
//! `config` crate.

/// Errors that can occur during MCP server operation.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Database connection failed.
    #[error("Database connection error: {0}")]
    Connection(String),

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

    /// Table isn't found in database.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// JSON serialization failed.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

impl From<AppError> for rmcp::model::ErrorData {
    fn from(e: AppError) -> Self {
        Self::internal_error(e.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_timeout_display_includes_elapsed_and_sql() {
        let err = AppError::QueryTimeout {
            elapsed_secs: 30.123_456,
            sql: "SELECT * FROM big_table".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("30.1"), "expected elapsed in message: {msg}");
        assert!(
            msg.contains("SELECT * FROM big_table"),
            "expected SQL in message: {msg}"
        );
    }
}
