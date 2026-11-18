//! Catalog-aware, column-level SQL lineage for `dbmcp`.
//!
//! [`analyze`] parses one statement for a [`Dialect`] and binds every column
//! it uses to physical `relation.column` sources taken from a caller-supplied
//! [`Catalog`]. The [`Lineage`] lists each output column's sources, every
//! other column use with its [`Role`] (`JOIN`, filter, `GROUP BY`, `ORDER BY`,
//! window), the relations touched, and the functions called.
//!
//! The crate never connects to a database. Names missing from the catalog
//! come back together as [`LineageError::Missing`]; the caller resolves them
//! on its own connection, inserts an [`Entry`] for each, and calls again.
//! Analysis is fail-closed: anything that cannot be bound, and any construct
//! lineage does not model, is an error.
//!
//! # Quickstart
//!
//! ```
//! use dbmcp_sql_lineage::{
//!     Catalog, CatalogColumn, Dialect, Entry, LineageError, Relation, RelationId, RelationKind,
//!     analyze,
//! };
//!
//! let sql = "SELECT u.email FROM users u WHERE u.id = 1";
//! let mut catalog = Catalog::new();
//! let lineage = loop {
//!     match analyze(sql, Dialect::Postgres, &catalog) {
//!         Err(LineageError::Missing(names)) => {
//!             for name in names {
//!                 // A real caller resolves `name` on its database connection.
//!                 let columns = vec![CatalogColumn::new("id"), CatalogColumn::new("email")];
//!                 let id = RelationId::new("app", "public", "users");
//!                 catalog.insert(name, Entry::Resolved(Relation::new(id, RelationKind::Table, columns)));
//!             }
//!         }
//!         other => break other?,
//!     }
//! };
//! assert_eq!(lineage.columns[0].name, "email");
//! assert_eq!(lineage.references.len(), 1);
//! # Ok::<(), LineageError>(())
//! ```

mod analyze;
mod bind;
mod catalog;
mod dialect;
mod error;
mod expr;
mod model;
mod resolve;
#[cfg(test)]
mod tests;

pub use crate::analyze::analyze;
pub use crate::catalog::{Catalog, CatalogColumn, Entry, Lexical, NamePart, Relation, WrittenName};
pub use crate::dialect::Dialect;
pub use crate::error::LineageError;
pub use crate::model::{
    FunctionUse, Lineage, Origin, OutputColumn, Reference, RelationId, RelationKind, RelationUse, Role, SourceRef,
    StatementKind,
};
