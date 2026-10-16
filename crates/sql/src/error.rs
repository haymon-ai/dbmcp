//! SQL-layer error types for validation, timeout, and query failures.

use rmcp::model::ErrorData;

/// `ErrorData::data` marker identifying [`SqlError::Connection`] failures.
const CONNECTION_KIND: &str = "connection";

/// Errors produced by SQL validation, identifier checking, and query execution.
#[derive(Debug, thiserror::Error)]
pub enum SqlError {
    /// Query blocked by read-only mode.
    #[error("Query blocked: only SELECT, SHOW, DESC, DESCRIBE, USE queries are allowed in read-only mode")]
    ReadOnlyViolation,

    /// `LOAD_FILE()` function blocked for security.
    #[error("Operation forbidden: LOAD_FILE() is not allowed for security reasons")]
    LoadFileBlocked,

    /// Dangerous function (file access, large objects, sequences, extensions) blocked.
    #[error("Operation forbidden: {0}() is not allowed for security reasons")]
    DangerousFunctionBlocked(String),

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

    /// The database could not be reached (I/O, TLS, protocol, or pool failure).
    ///
    /// Renders like [`SqlError::Query`]; only the classification differs.
    #[error("Database error: {0}")]
    Connection(String),

    /// Table not found in database.
    #[error("Table not found: {0}")]
    TableNotFound(String),
}

impl From<SqlError> for ErrorData {
    /// Converts to an internal error; connection failures carry a `kind` marker.
    fn from(e: SqlError) -> Self {
        let data = matches!(e, SqlError::Connection(_)).then(|| serde_json::json!({ "kind": CONNECTION_KIND }));
        Self::internal_error(e.to_string(), data)
    }
}

/// Returns whether `error` came from a [`SqlError::Connection`].
#[must_use]
pub fn is_connection_error(error: &ErrorData) -> bool {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("kind"))
        .is_some_and(|kind| kind == CONNECTION_KIND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_connection_errors_are_marked() {
        let connection = ErrorData::from(SqlError::Connection("reset".into()));
        assert_eq!(connection.message, "Database error: reset");
        assert!(is_connection_error(&connection));

        for error in [SqlError::Query("syntax".into()), SqlError::ReadOnlyViolation] {
            assert!(!is_connection_error(&ErrorData::from(error)));
        }
        assert!(!is_connection_error(&ErrorData::internal_error("other", None)));
    }
}
