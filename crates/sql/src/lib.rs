//! SQL sanitization, validation, and connection utilities.
//!
//! Provides [`sanitize`] helpers for quoting SQL identifiers,
//! [`validation`] for read-only query enforcement, [`pagination`]
//! for rewriting `SELECT` statements with a server-controlled
//! `LIMIT` / `OFFSET`, and the [`connection`] trait shared by every
//! backend, whose query methods run under a query timeout.

pub mod connection;
pub mod error;
pub mod pagination;
pub mod sanitize;
mod timeout;
pub mod validation;

pub use connection::Connection;
pub use error::{SqlError, is_connection_error};
pub use validation::StatementKind;
