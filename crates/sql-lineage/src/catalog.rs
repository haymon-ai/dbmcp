//! Catalog snapshot the caller supplies so relation names can be resolved.
//!
//! The crate never reads a database. The caller fills a [`Catalog`] with one
//! [`Entry`] per name that [`LineageError::Missing`](crate::LineageError::Missing)
//! reported, keyed by the name exactly as written, and calls again: a
//! [`Entry::Resolved`] relation, [`Entry::Vacant`] when nothing exists under
//! the name, or [`Entry::Absent`] when something exists that it cannot describe.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Origin, RelationId, RelationKind};

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogColumn {
    /// The column's actual name.
    pub name: String,
    /// Whether `*` expands to this column.
    pub star_visible: bool,
}

impl CatalogColumn {
    /// Creates a column that `*` expands to.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            star_visible: true,
        }
    }

    /// Creates a column that `*` skips (invisible, hidden, or system).
    pub fn hidden(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            star_visible: false,
        }
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
}

impl Relation {
    /// Creates a relation without a view body.
    #[must_use]
    pub fn new(id: RelationId, kind: RelationKind, columns: Vec<CatalogColumn>) -> Self {
        Self {
            id,
            kind,
            columns,
            view_sql: None,
        }
    }

    /// Sets the view body.
    #[must_use]
    pub fn with_view_sql(mut self, sql: impl Into<String>) -> Self {
        self.view_sql = Some(sql.into());
        self
    }
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
