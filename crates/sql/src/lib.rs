//! SQL validation, identifier, and connection utilities.
//!
//! Provides [`identifier`] helpers for quoting and validating SQL
//! identifiers, [`validation`] for read-only query enforcement,
//! [`timeout`] for query-level timeout wrapping, and the
//! [`connection`] trait shared by every backend.

pub mod connection;
pub mod identifier;
pub mod timeout;
pub mod validation;

pub use connection::Connection;
