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

    /// Database query execution failed.
    #[error("Database error: {0}")]
    Query(String),

    /// Table isn't found in database.
    #[error("Table not found: {0}")]
    TableNotFound(String),
}
