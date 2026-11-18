//! Name binding over one query level's visible relations and its outer levels.
//!
//! A [`Scope`] holds what one `SELECT`'s `FROM` list exposes: each visible
//! relation ([`Rel`]) for qualified lookups and `t.*`, and the combined
//! column list for unqualified lookups and `*` (`USING`/`NATURAL` keys merged
//! once). Lookups that find nothing locally continue in the parent scope.

use std::collections::BTreeSet;

use sqlparser::ast::Ident;

use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::SourceRef;

/// A column a relation or `FROM` list exposes.
#[derive(Debug, Clone)]
pub(crate) struct Col {
    /// The column's name, as the server knows it.
    pub name: String,
    /// Physical sources backing the column.
    pub sources: BTreeSet<SourceRef>,
    /// Whether `*` expands to the column.
    pub visible: bool,
}

/// A relation visible under a name.
#[derive(Debug, Clone)]
pub(crate) struct Rel {
    /// The name parts a qualifier may match: the alias, or the written name.
    pub path: Vec<String>,
    /// Columns in ordinal order.
    pub columns: Vec<Col>,
}

impl Rel {
    /// Returns whether a written qualifier names this relation.
    fn answers(&self, dialect: Dialect, qualifier: &[Ident]) -> bool {
        qualifier.len() <= self.path.len()
            && qualifier
                .iter()
                .rev()
                .zip(self.path.iter().rev())
                .all(|(q, p)| dialect.matches(q, p))
    }
}

/// The relations visible at one query level.
#[derive(Debug, Clone)]
pub(crate) struct Scope<'p> {
    /// Visible relations, in `FROM` order.
    pub rels: Vec<Rel>,
    /// Combined columns for `*` and unqualified lookups.
    pub cols: Vec<Col>,
    /// The enclosing query level, for correlated references.
    pub parent: Option<&'p Scope<'p>>,
}

impl<'p> Scope<'p> {
    pub(crate) fn new(parent: Option<&'p Scope<'p>>) -> Self {
        Self {
            rels: Vec::new(),
            cols: Vec::new(),
            parent,
        }
    }

    /// Binds a column reference, searching outer scopes innermost-first.
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when more than one candidate binds at
    /// the first level that has any; [`LineageError::ColumnNotFound`] when no
    /// level binds it.
    pub(crate) fn column(
        &self,
        dialect: Dialect,
        qualifier: &[Ident],
        name: &Ident,
    ) -> Result<BTreeSet<SourceRef>, LineageError> {
        let mut level = Some(self);
        while let Some(scope) = level {
            if qualifier.is_empty() {
                if let Some(col) = unique(dialect, &scope.cols, name, qualifier)? {
                    return Ok(col.sources.clone());
                }
            } else if let Some(rel) = scope.relation_here(dialect, qualifier)? {
                return unique(dialect, &rel.columns, name, qualifier)?
                    .map(|c| c.sources.clone())
                    .ok_or_else(|| not_found(qualifier, name));
            }
            level = scope.parent;
        }
        Err(not_found(qualifier, name))
    }

    /// Finds the relation a qualifier names, searching outer scopes.
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when several relations answer;
    /// [`LineageError::ColumnNotFound`] when none does.
    pub(crate) fn relation(&self, dialect: Dialect, qualifier: &[Ident]) -> Result<&Rel, LineageError> {
        let mut level = Some(self);
        while let Some(scope) = level {
            if let Some(rel) = scope.relation_here(dialect, qualifier)? {
                return Ok(rel);
            }
            level = scope.parent;
        }
        Err(LineageError::ColumnNotFound {
            column: format!("{}.*", dotted(qualifier)),
        })
    }

    /// Finds the relation a qualifier names at this level only.
    fn relation_here(&self, dialect: Dialect, qualifier: &[Ident]) -> Result<Option<&Rel>, LineageError> {
        let mut hits = self.rels.iter().filter(|r| r.answers(dialect, qualifier));
        let first = hits.next();
        if hits.next().is_some() {
            return Err(LineageError::AmbiguousColumn {
                column: dotted(qualifier),
            });
        }
        Ok(first)
    }
}

/// Returns the one column `name` matches, `None` for no match.
///
/// # Errors
/// [`LineageError::AmbiguousColumn`] when several columns match.
fn unique<'c>(
    dialect: Dialect,
    cols: &'c [Col],
    name: &Ident,
    qualifier: &[Ident],
) -> Result<Option<&'c Col>, LineageError> {
    let mut hits = cols.iter().filter(|c| dialect.matches(name, &c.name));
    let first = hits.next();
    if hits.next().is_some() {
        return Err(LineageError::AmbiguousColumn {
            column: written(qualifier, name),
        });
    }
    Ok(first)
}

fn not_found(qualifier: &[Ident], name: &Ident) -> LineageError {
    LineageError::ColumnNotFound {
        column: written(qualifier, name),
    }
}

fn written(qualifier: &[Ident], name: &Ident) -> String {
    if qualifier.is_empty() {
        name.value.clone()
    } else {
        format!("{}.{}", dotted(qualifier), name.value)
    }
}

fn dotted(idents: &[Ident]) -> String {
    idents.iter().map(|i| i.value.as_str()).collect::<Vec<_>>().join(".")
}
