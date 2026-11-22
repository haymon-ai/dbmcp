//! What a backend reports for a written name: the relation plus its write facts.
//!
//! Lineage analysis reads only names, `*` visibility, and view bodies; the
//! planner also reads what the server can write on its own (defaults,
//! generated and on-update columns, triggers, cascades) and the inheritance
//! tree. Every fact is a field without a default, so a backend states each
//! one; when it cannot know a fact it states the value that refuses (`true`).

use std::collections::BTreeSet;

use dbmcp_sql_lineage as lineage;
use dbmcp_sql_lineage::{RelationId, RelationKind, SourceRef};

/// What the backend found for a written name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// The name resolves to this relation.
    Resolved(Relation),
    /// Something exists under the name that the backend cannot describe.
    ///
    /// Always an error, even under `IF EXISTS`; use [`Entry::Vacant`] for a
    /// free name.
    Absent,
    /// Nothing exists under the name; a relation created under it gets this id.
    ///
    /// A `CREATE` target takes the id; `DROP`/`TRUNCATE … IF EXISTS` has no
    /// target; anywhere else it is an unknown relation, like [`Entry::Absent`].
    Vacant(RelationId),
}

/// A relation the backend resolved a written name to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// Canonical identity.
    pub id: RelationId,
    /// What kind of relation it is.
    pub kind: RelationKind,
    /// Columns in ordinal order.
    pub columns: Vec<CatalogColumn>,
    /// The view's query, without `CREATE VIEW`, for views.
    pub view_sql: Option<String>,
    /// The relation's inheritance tree; empty where the backend has none.
    ///
    /// An empty tree keeps rules of related relations from reaching this one,
    /// so a backend with inheritance must read it.
    pub family: Family,
    /// Whether it or a `PostgreSQL` descendant has user triggers or non-`SELECT` rules.
    pub triggers: bool,
    /// Referential action edges reachable below the relation, firing or not.
    ///
    /// Every action edge of the relation (and of each `PostgreSQL`
    /// descendant), then every action edge of each child table so reached,
    /// transitively. The planner decides which edges a write fires; an edge
    /// missing here is one it cannot follow (fail open).
    pub cascades: Vec<Cascade>,
}

/// A column of a catalog relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    /// The column's actual name.
    pub name: String,
    /// Whether `*` expands to this column.
    pub star_visible: bool,
    /// Whether the server can fill the column itself: any default, identity, sequence, or auto-increment.
    pub default: bool,
    /// Whether the column is generated (stored or virtual).
    pub generated: bool,
    /// Whether an `UPDATE` sets the column itself (`ON UPDATE CURRENT_TIMESTAMP`).
    pub on_update: bool,
}

/// One referential action edge from a parent column to a child column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cascade {
    /// The referenced column.
    pub parent: SourceRef,
    /// The referencing column the action writes, or whose rows it removes.
    pub child: SourceRef,
    /// What the action writes into the child.
    pub action: CascadeAction,
    /// Whether a `DELETE` of the parent fires it (otherwise an `UPDATE` of `parent` does).
    pub on_delete: bool,
    /// Whether the child relation has a user trigger or a non-`SELECT` rule.
    pub triggers: bool,
}

/// What a referential action writes into the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeAction {
    /// `CASCADE` on update: copies the parent's new value.
    Copy,
    /// `SET NULL`.
    Null,
    /// `SET DEFAULT`: the child column's default.
    Default,
    /// `CASCADE` on delete: removes child rows.
    Delete,
}

/// A relation's inheritance tree, as relation identities only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Family {
    /// Whether the relation itself is a partitioned table.
    pub partitioned: bool,
    /// Every ancestor, transitively.
    pub ancestors: BTreeSet<RelationId>,
    /// Every descendant, transitively, each with its own ancestors.
    pub descendants: Vec<Member>,
}

impl Family {
    /// Returns every relation whose rules can reach the family's owner.
    ///
    /// Its ancestors, its descendants, and every descendant's ancestors.
    #[must_use]
    pub fn relations(&self) -> BTreeSet<&RelationId> {
        let mut relations: BTreeSet<&RelationId> = self.ancestors.iter().collect();
        for member in &self.descendants {
            relations.insert(&member.id);
            relations.extend(&member.ancestors);
        }
        relations
    }
}

/// A descendant in a [`Family`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The descendant's identity.
    pub id: RelationId,
    /// Whether it is a partitioned table, which holds no rows itself.
    pub partitioned: bool,
    /// Every ancestor of this member, transitively, from every parent it has.
    pub ancestors: BTreeSet<RelationId>,
}

impl Relation {
    /// Returns what lineage analysis reads: identity, kind, column names, and view body.
    pub(crate) fn lineage(&self) -> lineage::Relation {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                if column.star_visible {
                    lineage::CatalogColumn::new(&column.name)
                } else {
                    lineage::CatalogColumn::hidden(&column.name)
                }
            })
            .collect();
        lineage::Relation {
            id: self.id.clone(),
            kind: self.kind,
            columns,
            view_sql: self.view_sql.clone(),
        }
    }
}
