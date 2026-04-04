//! SQL validation and identifier utilities for database backends.
//!
//! Provides [`identifier`] helpers for quoting and validating SQL
//! identifiers, and [`validation`] for read-only query enforcement.

pub mod identifier;
pub mod validation;
