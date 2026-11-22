//! Catalog snapshot the caller supplies so relation names can be resolved.
//!
//! The crate never reads a database. The caller fills a [`Catalog`] with one
//! [`Entry`] per name that [`LineageError::Missing`](crate::LineageError::Missing)
//! reported, keyed by the name exactly as written, and calls again: a
//! [`Entry::Resolved`] relation, [`Entry::Vacant`] when nothing exists under
//! the name, or [`Entry::Absent`] when something exists that it cannot describe.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Origin, RelationId, RelationKind, SourceRef};

/// One part of a relation name as written in SQL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NamePart {
    /// The identifier text without quotes.
    pub value: String,
    /// Whether the identifier was quoted.
    pub quoted: bool,
}

impl NamePart {
    /// Creates an unquoted name part.
    pub fn bare(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            quoted: false,
        }
    }

    /// Creates a quoted name part.
    pub fn quoted(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            quoted: true,
        }
    }
}

/// A relation name as written, plus where it was written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WrittenName {
    /// The dotted parts, outermost qualifier first.
    pub parts: Vec<NamePart>,
    /// Whether the name appeared in the query itself or inside a view body.
    pub context: Origin,
}

impl WrittenName {
    /// Creates a name written in the query itself.
    #[must_use]
    pub fn query(parts: Vec<NamePart>) -> Self {
        Self {
            parts,
            context: Origin::Query,
        }
    }
}

impl fmt::Display for WrittenName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, part) in self.parts.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            if part.quoted {
                write!(f, "\"{}\"", part.value)?;
            } else {
                f.write_str(&part.value)?;
            }
        }
        Ok(())
    }
}

/// Whether the server tokenizes SQL the same way the parser does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lexical {
    /// The server lexes SQL like the parser.
    #[default]
    Matches,
    /// The server lexes SQL differently; the reason names the setting.
    Mismatch(String),
}

/// A column of a catalog relation.
///
/// Analysis reads only the name and `*` visibility; the other facts carry
/// catalog knowledge to the caller's planner. Both constructors set every
/// fact to `true` (fail closed) until [`CatalogColumn::with_facts`] sets them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl CatalogColumn {
    /// Creates a column that `*` expands to, with every fact `true`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            star_visible: true,
            default: true,
            generated: true,
            on_update: true,
        }
    }

    /// Creates a column that `*` skips (invisible, hidden, or system), with every fact `true`.
    pub fn hidden(name: impl Into<String>) -> Self {
        Self {
            star_visible: false,
            ..Self::new(name)
        }
    }

    /// Sets whether the column has a default, is generated, and is set by `UPDATE`.
    #[must_use]
    pub fn with_facts(mut self, default: bool, generated: bool, on_update: bool) -> Self {
        self.default = default;
        self.generated = generated;
        self.on_update = on_update;
        self
    }
}

/// A relation the backend resolved a written name to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    /// Canonical identity.
    pub id: RelationId,
    /// What kind of relation it is.
    pub kind: RelationKind,
    /// Columns in ordinal order.
    pub columns: Vec<CatalogColumn>,
    /// The view's query, without `CREATE VIEW`, for views.
    pub view_sql: Option<String>,
    /// The relation's inheritance tree; empty unless the backend reads one.
    ///
    /// Analysis ignores it; it carries catalog knowledge to the caller's planner.
    pub family: Family,
    /// Whether it or a `PostgreSQL` descendant has user triggers or non-`SELECT` rules.
    ///
    /// Analysis ignores it, like `cascades`.
    pub triggers: bool,
    /// Referential actions that change rows of other tables, transitively.
    pub cascades: Vec<Cascade>,
}

impl Relation {
    /// Creates a relation without a view body, inheritance tree, or cascades.
    ///
    /// It counts as having triggers (fail closed) until
    /// [`Relation::with_triggers`] says otherwise.
    #[must_use]
    pub fn new(id: RelationId, kind: RelationKind, columns: Vec<CatalogColumn>) -> Self {
        Self {
            id,
            kind,
            columns,
            view_sql: None,
            family: Family::default(),
            triggers: true,
            cascades: Vec::new(),
        }
    }

    /// Sets the view body.
    #[must_use]
    pub fn with_view_sql(mut self, sql: impl Into<String>) -> Self {
        self.view_sql = Some(sql.into());
        self
    }

    /// Sets the inheritance tree.
    #[must_use]
    pub fn with_family(mut self, family: Family) -> Self {
        self.family = family;
        self
    }

    /// Sets whether the relation has a user trigger or a non-`SELECT` rule.
    #[must_use]
    pub fn with_triggers(mut self, triggers: bool) -> Self {
        self.triggers = triggers;
        self
    }

    /// Sets the referential actions that change rows of other tables.
    #[must_use]
    pub fn with_cascades(mut self, cascades: Vec<Cascade>) -> Self {
        self.cascades = cascades;
        self
    }
}

/// One referential action edge from a parent column to a child column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The descendant's identity.
    pub id: RelationId,
    /// Whether it is a partitioned table, which holds no rows itself.
    pub partitioned: bool,
    /// Every ancestor of this member, transitively, from every parent it has.
    pub ancestors: BTreeSet<RelationId>,
}

/// What the backend found for a written name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A snapshot of relations keyed by written name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    lexical: Lexical,
    relations: BTreeMap<WrittenName, Entry>,
}

impl Catalog {
    /// Creates an empty catalog whose lexing matches the parser.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the lexical state.
    #[must_use]
    pub fn lexical(&self) -> &Lexical {
        &self.lexical
    }

    /// Sets the lexical state.
    pub fn set_lexical(&mut self, lexical: Lexical) {
        self.lexical = lexical;
    }

    /// Inserts or replaces the entry for a written name.
    pub fn insert(&mut self, name: WrittenName, entry: Entry) {
        self.relations.insert(name, entry);
    }

    /// Returns the entry for a written name.
    #[must_use]
    pub fn get(&self, name: &WrittenName) -> Option<&Entry> {
        self.relations.get(name)
    }
}
