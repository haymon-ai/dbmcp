//! SQL sanitization, validation, and connection utilities.
//!
//! Provides [`sanitize`] helpers for quoting SQL identifiers,
//! [`validation`] for read-only query enforcement, [`pagination`]
//! for rewriting `SELECT` statements with a server-controlled
//! `LIMIT` / `OFFSET`, [`timeout`] for query-level timeout wrapping,
//! and the [`connection`] trait shared by every backend.

pub mod connection;
pub mod error;
pub mod pagination;
pub mod sanitize;
pub mod timeout;
pub mod validation;

pub use connection::Connection;
pub use error::SqlError;
pub use validation::StatementKind;
