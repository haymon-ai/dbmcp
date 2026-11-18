//! Column-level PII policy planner.
//!
//! [`plan`] turns a statement's [`Lineage`] and the `pii.rules` of a
//! [`PiiConfig`] into a [`Plan`]: one [`Position`] per output column saying
//! how its values are redacted. It rejects the statement instead, before it
//! runs, when any gate fails. The gates run in this order, all fail closed:
//!
//! 1. Lineage errors reject; callers map them with [`reject_lineage`].
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
//! [`Lineage`]: dbmcp_sql_lineage::Lineage
//! [`PiiConfig`]: dbmcp_config::PiiConfig

mod deny;
mod plan;
mod rejection;
#[cfg(test)]
mod tests;

pub use self::plan::{Plan, Position, plan, touches_ruled};
pub use self::rejection::{CATALOG_ROUNDS, LineageClass, Rejection, reject_lineage};
