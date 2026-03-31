//! Database backend trait, SQL validation, and identifier utilities.
//!
//! Defines the [`DatabaseBackend`] trait that all database backends must
//! implement, along with shared validation and identifier checking.

pub mod identifier;
pub mod traits;
pub mod validation;

pub use traits::DatabaseBackend;
