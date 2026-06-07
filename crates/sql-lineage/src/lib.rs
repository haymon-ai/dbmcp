//! Column-level SQL lineage extraction for `dbmcp`.
//!
//! Given a single SQL statement and a `sqlparser`
//! [`Dialect`](sqlparser::dialect::Dialect),
//! [`extract`] returns a [`Lineage`] mapping each output column to the physical
//! `table.column` sources that produced it. Read queries and write statements
//! (`INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE AS SELECT`, `MERGE`) are both
//! handled; writes also record a [`Lineage::target`]. Resolution is fail-closed:
//! any column that cannot be bound to a concrete source aborts the whole
//! extraction. The crate never connects to or executes against a database — it
//! analyzes SQL text only.
//!
//! Wildcards over base tables (`SELECT *`) and unqualified columns in
//! multi-table joins cannot be resolved without a column catalog, so they fail
//! closed; qualified references, single-table queries, and wildcards over
//! CTEs/derived tables resolve normally.
//!
//! # Quickstart
//!
//! ```
//! use dbmcp_sql_lineage::extract;
//! use sqlparser::dialect::PostgreSqlDialect;
//!
//! let lineage = extract("SELECT u.id, u.email FROM users u", &PostgreSqlDialect {})?;
//! assert_eq!(lineage.columns[1].name.as_deref(), Some("email"));
//! # Ok::<(), dbmcp_sql_lineage::LineageError>(())
//! ```

#![deny(missing_docs)]

mod error;
mod extract;
mod model;
mod resolve;
mod scope;
mod statement;

pub use crate::error::LineageError;
pub use crate::extract::extract;
pub use crate::model::{Lineage, OutputColumn, SourceRef, TableRef, TableRefBuilder};
