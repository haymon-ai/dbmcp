//! Public lineage result types.
//!
//! Every name here is canonical, as the catalog supplied it; comparison is
//! exact.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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
    /// Where it was named: the statement, or the view whose body names it.
    ///
    /// For a view, this is its parent-view edge; walk it to rebuild a chain.
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

/// One column in a statement's result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputColumn {
    /// Best-effort output name; informational only.
    pub name: String,
    /// 0-based position in the result set.
    pub position: usize,
    /// Physical columns whose values flow into this one; empty for constants.
    pub sources: BTreeSet<SourceRef>,
}

/// The kind of statement analyzed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StatementKind {
    /// A query: `SELECT`, set operation, or `VALUES`.
    Query,
}

/// The lineage of a single statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    /// Output columns in result order.
    pub columns: Vec<OutputColumn>,
    /// Columns used outside the projection, by role.
    pub references: BTreeSet<Reference>,
    /// Catalog relations touched.
    pub relations: BTreeSet<RelationUse>,
    /// Functions called.
    pub functions: BTreeSet<FunctionUse>,
    /// Write target; `None` for a query.
    pub target: Option<RelationId>,
    /// The statement kind.
    pub statement: StatementKind,
}
