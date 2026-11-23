//! Column-rules (`privacy.rules`) policy planner for `dbmcp`.
//!
//! Backends reach SQL lineage only through this crate. An [`Analysis`]
//! analyzes one statement, asking the backend for each relation it names
//! that the catalog lacks; the backend looks the names up on its own
//! connection and passes back an [`Entry`] for each. The finished
//! [`Analyzed`] statement then plans with [`Analyzed::plan`] under the
//! `privacy.rules` of a [`PrivacyConfig`]: one [`Position`] per output column saying
//! how its values are redacted, bound to those rules as a [`Planned`], which
//! checks, withholds errors for, and redacts the result. Planning rejects the
//! statement instead, before it runs, when any gate fails. The gates run in this order, all fail closed:
//!
//! 1. Lineage errors reject, and so do names still missing after
//!    [`CATALOG_ROUNDS`] catalog rounds.
//! 2. Functions and cast target types missing from the dialect's built-in tables reject.
//! 3. Denied relations (statistics, statement text, raw pages, virtual
//!    tables) reject.
//! 4. Denied statements (`SHOW WARNINGS` and friends, every `PostgreSQL`
//!    `SHOW`) reject, also under `EXPLAIN`.
//! 5. `EXPLAIN` of a statement touching a ruled relation rejects, unless it
//!    is a plain JSON or `SQLite` query-plan form (the forms `explainQuery`
//!    builds, read from the statement), whose plan [`Planned::redact`] prunes
//!    to value-free keys; any `ANALYZE` on one rejects too.
//! 6. `CREATE INDEX` on a ruled relation rejects: a unique key leaks values
//!    through duplicate-key errors.
//! 7. A ruled column used in `JOIN`, `WHERE`, `GROUP BY`, `ORDER BY`, a
//!    window, `DISTINCT` or a deduplicating set operation, or a
//!    set-returning function's arguments rejects, in the statement or in a
//!    view body it reads.
//! 8. A [`Flow::Bare`](dbmcp_sql_lineage::Flow::Bare) output column takes
//!    the `(operator, entity)` of its ruled sources; two different pairs
//!    reject. A `hash` pair holds only when every source is hashed under it;
//!    otherwise `hash` becomes `replace` before the pairs are compared. Any
//!    other output column over a ruled source reads as its entity's
//!    placeholder, `NULL` included; its ruled sources must share the entity.
//! 9. A bare create target resolving outside the creation schema rejects.
//! 10. A write copying ruled values needs a bare copy into a target column
//!     ruled with the same pair and named so a rule can match it; a computed
//!     ruled value is laundering into any target. `CREATE VIEW` is exempt.
//! 11. A `hash`-ruled column written by a statement takes only same-hashed
//!     bare copies, `NULL`, or `DEFAULT` when it has no default; a write that
//!     lets the server fill one (an omitted column's default, a generated or
//!     on-update column) rejects, and so does a fired cascade that sets a
//!     `hash`-ruled child column to its default or copies into it from a
//!     parent not hashed under the same pair.
//! 12. While any `hash` rule exists, a write to a relation with triggers or
//!     rules rejects, and so does one firing a cascade into such a table.
//! 13. An `UPDATE` or `MERGE` through a partitioned table rejects while its
//!     leaf partitions read a ruled column under different pairs: a row
//!     moved between them would be read back under other rules.
//!
//! Each column matches the rule whose folded key equals its folded
//! `catalog.schema.table.column`. Every read-side lookup also covers the
//! relation's `PostgreSQL` inheritance [`Family`]: a column is ruled by the
//! rules on the same-named column of its ancestors, its descendants, and
//! their other ancestors, never its siblings, so rows read through a parent
//! follow the rules of the member they live in. The planner is pure and sync: it never
//! touches a database. Rejection messages name only catalog identifiers,
//! never a rule's name, entity, or operator, nor a value.
//!
//! [`PrivacyConfig`]: dbmcp_config::PrivacyConfig
//! [`Position`]: dbmcp_privacy::cells::Position

mod analysis;
mod catalog;
mod deny;
mod enforce;
mod explain;
mod plan;
mod planned;
mod rejection;
mod relation;
#[cfg(test)]
mod tests;

pub use dbmcp_sql_lineage::{NamePart, Origin, RelationId, RelationKind, SourceRef, StatementKind, WrittenName};

pub use self::analysis::{Analysis, Analyzed, Step};
pub use self::catalog::{Cascade, CascadeAction, CatalogColumn, Entry, Family, Member, Relation};
pub use self::enforce::{Access, Admitted, Enforced, Executed, Run, Stop, admit, settle};
pub use self::planned::{Planned, Unfit};
pub use self::rejection::{CATALOG_ROUNDS, LineageClass, Rejection};
pub use self::relation::RelationRef;
