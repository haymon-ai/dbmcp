//! SQL sanitization, validation, and connection utilities.
//!
//! Provides [`sanitize`] helpers for quoting SQL identifiers,
//! [`validation`] for read-only query enforcement, [`pagination`]
//! for rewriting `SELECT` statements with a server-controlled
//! `LIMIT` / `OFFSET`, the [`connection`] trait shared by every
//! backend, whose query methods run under a query timeout, and
//! [`support`] for the support tier of a server version.

pub mod connection;
pub mod error;
pub mod pagination;
pub mod sanitize;
pub mod support;
mod timeout;
pub mod validation;

pub use connection::Connection;
pub use dbmcp_sql_lineage::Dialect;
pub use error::{SqlError, is_connection_error};
pub use validation::StatementKind;
