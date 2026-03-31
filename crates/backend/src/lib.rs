//! Shared backend utilities: error types, SQL validation, identifier checking, and request types.
//!
//! Provides [`AppError`] for error handling, validation utilities, and shared
//! MCP tool request types used by all database backend implementations.

pub mod error;
pub mod identifier;
pub mod types;
pub mod validation;

pub use error::AppError;
pub use types::{CreateDatabaseRequest, GetTableSchemaRequest, ListTablesRequest, QueryRequest};
