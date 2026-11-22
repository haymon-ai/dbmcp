//! Public lineage result types.
//!
//! Every name here is canonical, as the catalog supplied it; comparison is
//! exact.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::explain::ExplainForm;
use crate::show::Show;

/// Canonical identity of a relation, as the backend reports it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationId {
    /// Catalog or database.
    pub catalog: String,
    /// Schema.
    pub schema: String,
    /// Relation name.
    pub name: String,
}

impl RelationId {
    /// Creates a relation identity.
    pub fn new(catalog: impl Into<String>, schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            catalog: catalog.into(),
            schema: schema.into(),
            name: name.into(),
        }
    }
}

/// A physical column of a catalog relation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRef {
    /// The relation.
    pub relation: RelationId,
    /// The column's actual name.
    pub column: String,
}

impl SourceRef {
    /// Creates a source reference.
    pub fn new(relation: RelationId, column: impl Into<String>) -> Self {
        Self {
            relation,
            column: column.into(),
        }
    }
}

/// Where a reference, relation, or function use came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Origin {
    /// The analyzed statement itself.
    Query,
    /// The body of this view, the innermost one when views nest.
    View(RelationId),
}

/// The clause a non-projection column reference sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Role {
    /// `JOIN … ON`, `USING`, or `NATURAL` keys.
    Join,
    /// `WHERE`, `HAVING`, `LIMIT`/`OFFSET`, or DML `WHERE`.
    Filter,
    /// `GROUP BY` or `DISTINCT ON`.
    GroupBy,
    /// `ORDER BY` of a query.
    Sort,
    /// `PARTITION BY` / `ORDER BY` inside `OVER`.
    Window,
}

/// A column used outside the projection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Reference {
    /// The physical column.
    pub source: SourceRef,
    /// The clause it sits in.
    pub role: Role,
    /// Where it was written.
    pub origin: Origin,
}

/// The kind of a catalog relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    /// Ordinary table.
    Table,
    /// View.
    View,
    /// Materialized view.
    MatView,
    /// Foreign table.
    Foreign,
    /// System or catalog relation.
    System,
    /// Virtual or shadow table (`SQLite`).
    Virtual,
}

/// A catalog relation the statement touches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationUse {
    /// Canonical identity.
    pub id: RelationId,
    /// Relation kind.
    pub kind: RelationKind,
    /// Where it was named.
    pub origin: Origin,
}

/// A function the statement calls.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FunctionUse {
    /// Function name, case-folded per dialect.
    pub name: String,
    /// Whether the dialect's function table lists it.
    pub known: bool,
    /// Where it was called.
    pub origin: Origin,
}

/// A type the statement casts to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TypeUse {
    /// Type name, case-folded per dialect.
    pub name: String,
    /// Whether the dialect's type table lists it.
    pub known: bool,
    /// Where it was cast to.
    pub origin: Origin,
}

/// How a column's value reaches it from its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flow {
    /// A literal `NULL`, possibly parenthesized or cast.
    Null,
    /// The `DEFAULT` keyword.
    Default,
    /// A column reference passed through unchanged.
    Bare,
    /// Any other expression.
    Derived,
}

impl Flow {
    /// Combines the flows of two values that land in one column.
    ///
    /// `Null` is the identity; two `Bare` or two `Default` flows keep their
    /// kind; every other pair is `Derived`.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Null, flow) | (flow, Self::Null) => flow,
            (Self::Bare, Self::Bare) => Self::Bare,
            (Self::Default, Self::Default) => Self::Default,
            _ => Self::Derived,
        }
    }
}

/// One column in a statement's result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputColumn {
    /// Best-effort output name; informational only.
    pub name: String,
    /// 0-based position in the result set.
    pub position: usize,
    /// Physical columns whose values flow into this one; empty for constants.
    pub sources: BTreeSet<SourceRef>,
    /// How the value reaches the column from its sources.
    pub flow: Flow,
}

/// The kind of statement analyzed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StatementKind {
    /// A query: `SELECT`, set operation, or `VALUES`.
    Query,
    /// `INSERT` (or `REPLACE`), with `VALUES`, a query, or `SET`.
    Insert,
    /// `UPDATE`, including `PostgreSQL` `FROM` and `MySQL` joined tables.
    Update,
    /// `DELETE`, including `USING` and `MySQL` joined tables.
    Delete,
    /// `MERGE`.
    Merge,
    /// `CREATE TABLE` without a query.
    CreateTable,
    /// `CREATE TABLE … AS` or `PostgreSQL` `SELECT … INTO`.
    CreateTableAs,
    /// `CREATE VIEW`.
    CreateView,
    /// `CREATE MATERIALIZED VIEW`.
    CreateMaterializedView,
    /// `DROP TABLE`, `VIEW`, `MATERIALIZED VIEW`, or `INDEX`.
    Drop,
    /// `TRUNCATE`.
    Truncate,
    /// `CREATE INDEX`.
    CreateIndex,
    /// `SHOW`, classified by its leading keywords.
    Show(Show),
    /// `DESCRIBE`, `DESC`, or `EXPLAIN` of a table.
    Describe,
    /// `EXPLAIN` in its [`ExplainForm`] of the inner statement, whose lineage this one carries.
    ///
    /// The lineage's columns are the explained statement's, not the plan's
    /// result set, so they do not match output positions.
    Explain(ExplainForm, Box<StatementKind>),
}

/// A column a statement writes, with the physical columns feeding it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetColumn {
    /// The target column's actual name, or the new column's name on create.
    pub name: String,
    /// Physical columns whose values flow into the target column.
    pub sources: BTreeSet<SourceRef>,
    /// How the written values reach the column, merged over every write.
    pub flow: Flow,
    /// Whether `name` is the name the server stores.
    ///
    /// `false` only for a created column from an unaliased expression or a
    /// `VALUES` list, whose stored name is engine-specific: a rule cannot
    /// name it reliably.
    pub named: bool,
}

/// The lineage of a single statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    /// Output columns in result order; for writes, the `RETURNING` list; for `EXPLAIN`, the explained statement's.
    pub columns: Vec<OutputColumn>,
    /// Columns used outside the projection, by role.
    pub references: BTreeSet<Reference>,
    /// Catalog relations touched, the target included.
    pub relations: BTreeSet<RelationUse>,
    /// Functions called.
    pub functions: BTreeSet<FunctionUse>,
    /// Cast target types.
    pub types: BTreeSet<TypeUse>,
    /// The relation written, created, dropped, truncated, or indexed; `None` for a query.
    pub target: Option<RelationId>,
    /// Columns of `target` written, in first-write order, sources merged.
    pub writes: Vec<TargetColumn>,
    /// The statement kind.
    pub statement: StatementKind,
}
