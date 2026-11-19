//! Column-rules (`pii.rules`) policy planner for `dbmcp`.
//!
//! Backends reach SQL lineage only through this crate. An [`Analysis`]
//! analyzes one statement, asking the backend for each relation it names
//! that the catalog lacks; the backend looks the names up on its own
//! connection and passes back an [`Entry`] for each. The finished
//! [`Analyzed`] statement then plans with [`Analyzed::plan`] under the
//! `pii.rules` of a [`PiiConfig`]: one [`Position`] per output column saying
//! how its values are redacted, bound to those rules as a [`Planned`] that
//! also checks the origins the server reports for each result column. It rejects the statement instead, before it
//! runs, when any gate fails. The gates run in this order, all fail closed:
//!
//! 1. Lineage errors reject, and so do names still missing after
//!    [`CATALOG_ROUNDS`] catalog rounds.
//! 2. Functions missing from the dialect's built-in table reject.
//! 3. Denied relations (statistics, statement text, raw pages, virtual
//!    tables) reject.
//! 4. Denied statements (`SHOW WARNINGS` and friends, every `PostgreSQL`
//!    `SHOW`) reject, also under `EXPLAIN`.
//! 5. `EXPLAIN` of a statement touching a ruled relation rejects.
//! 6. `CREATE INDEX` on a ruled relation rejects: a unique key leaks values
//!    through duplicate-key errors.
//! 7. A ruled column used in `JOIN`, `WHERE`, `GROUP BY`, `ORDER BY`, or a
//!    window rejects, in the statement or in a view body it reads.
//! 8. Each output column takes the `(operator, entity)` of its ruled
//!    sources; two different pairs reject.
//! 9. A write copying ruled values needs a target column ruled with the same
//!    pair and named so a rule can match it; `CREATE VIEW` is exempt.
//!
//! Each column matches the rule whose folded key equals its folded
//! `catalog.schema.table.column`. The planner is pure and sync: it never
//! touches a database. Rejection messages name only catalog identifiers,
//! never a rule's name, entity, or operator, nor a value.
//!
//! [`PiiConfig`]: dbmcp_config::PiiConfig
//! [`Position`]: dbmcp_pii::cells::Position

mod analysis;
mod deny;
mod plan;
mod planned;
mod rejection;
#[cfg(test)]
mod tests;

pub use dbmcp_sql_lineage::{
    CatalogColumn, Entry, NamePart, Origin, Relation, RelationId, RelationKind, StatementKind, WrittenName,
};

pub use self::analysis::{Analysis, Analyzed, Step};
pub use self::planned::Planned;
pub use self::rejection::{CATALOG_ROUNDS, LineageClass, Rejection};
