//! SQL sanitization, validation, and connection utilities.
//!
//! Provides [`sanitize`] helpers for quoting and validating SQL
//! identifiers and literals, [`validation`] for read-only query
//! enforcement, [`timeout`] for query-level timeout wrapping, and
//! the [`connection`] trait shared by every backend.

pub mod connection;
pub mod sanitize;
pub mod timeout;
pub mod validation;

pub use connection::Connection;
