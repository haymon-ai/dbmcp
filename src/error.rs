//! Application error types for the MCP server.
//!
//! Defines [`AppError`] with variants for configuration, connection,
//! security validation, and query execution failures.

/// Errors that can occur during MCP server operation.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    /// Configuration is invalid or missing.
    #[error("Configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    /// Database connection failed.
    #[error("Database connection error: {0}")]
    Connection(String),

    /// Query blocked by read-only mode.
    #[error(
        "Query blocked: only SELECT, SHOW, DESC, DESCRIBE, USE queries are allowed in read-only mode"
    )]
    ReadOnlyViolation,

    /// `LOAD_FILE()` function blocked for security.
    #[error("Operation forbidden: LOAD_FILE() is not allowed for security reasons")]
    LoadFileBlocked,

    /// INTO OUTFILE/DUMPFILE blocked for security.
    #[error(
        "Operation forbidden: SELECT INTO OUTFILE/DUMPFILE is not allowed for security reasons"
    )]
    IntoOutfileBlocked,

    /// Multiple SQL statements blocked.
    #[error("Query blocked: only single statements are allowed")]
    MultiStatement,

    /// Invalid database or table name identifier.
    #[error(
        "Invalid database/table name '{0}': must contain only alphanumeric characters and underscores"
    )]
    InvalidIdentifier(String),

    /// Database query execution failed.
    #[error("Database error: {0}")]
    Query(String),

    /// Table not found in database.
    #[error("Table not found: {0}")]
    TableNotFound(String),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Connection(e.to_string())
    }
}
