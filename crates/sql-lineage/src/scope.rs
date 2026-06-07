//! Name resolution over a query scope's visible relations.
//!
//! A [`Scope`] holds the relations visible in one `SELECT`'s `FROM`/`JOIN`
//! list. Physical tables resolve explicitly-referenced columns directly;
//! CTE/derived relations expose pre-resolved output columns
//! ([`Virtual`](RelExpose::Virtual)) and can be wildcard-expanded.

use std::collections::BTreeSet;

use crate::error::LineageError;
use crate::model::{SourceRef, TableRef};

/// A resolved output column of a CTE or derived table.
#[derive(Debug, Clone)]
pub(crate) struct VirtCol {
    /// Output name, if any.
    pub name: Option<String>,
    /// Physical sources backing this column.
    pub sources: BTreeSet<SourceRef>,
}

/// What a visible relation exposes for column resolution.
#[derive(Debug, Clone)]
pub(crate) enum RelExpose {
    /// A physical base table. Its column list is unknown, so wildcards cannot
    /// be expanded and unqualified columns cannot be disambiguated against it.
    Physical {
        /// The physical table.
        table: TableRef,
    },
    /// A CTE or derived table exposing pre-resolved output columns.
    Virtual {
        /// The relation's output columns.
        columns: Vec<VirtCol>,
    },
}

/// Expanded wildcard columns: each entry is an optional name and its sources.
pub(crate) type ExpandedColumns = Vec<(Option<String>, BTreeSet<SourceRef>)>;

/// A relation visible in a scope under a name (alias or table name).
#[derive(Debug, Clone)]
pub(crate) struct Relation {
    /// The name this relation is referenced by within the scope.
    pub visible: String,
    /// What the relation exposes.
    pub expose: RelExpose,
}

/// The relations visible within one `SELECT` scope.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    /// Visible relations, in `FROM`/`JOIN` order.
    relations: Vec<Relation>,
}

impl Scope {
    /// Adds a visible relation referenced by `visible` (alias or table name).
    pub(crate) fn push(&mut self, visible: String, expose: RelExpose) {
        self.relations.push(Relation { visible, expose });
    }

    /// Returns the physical (base-table) relations visible in this scope.
    pub(crate) fn physical_tables(&self) -> BTreeSet<TableRef> {
        self.relations
            .iter()
            .filter_map(|r| match &r.expose {
                RelExpose::Physical { table } => Some(table.clone()),
                RelExpose::Virtual { .. } => None,
            })
            .collect()
    }

    /// Resolves a (optionally qualified) column reference to its sources.
    ///
    /// # Errors
    /// Returns [`LineageError::UnresolvedColumn`] / [`LineageError::UnknownTable`]
    /// when the reference is unknown or ambiguous (fail closed).
    pub(crate) fn resolve_column(
        &self,
        qualifier: Option<&str>,
        column: &str,
    ) -> Result<BTreeSet<SourceRef>, LineageError> {
        if let Some(q) = qualifier {
            return resolve_in_relation(self.relation_named(q)?, column);
        }

        if self.relations.len() == 1 {
            return resolve_in_relation(&self.relations[0], column);
        }

        let mut candidates: Vec<BTreeSet<SourceRef>> = Vec::new();
        let mut has_physical = false;
        for rel in &self.relations {
            match &rel.expose {
                RelExpose::Physical { .. } => has_physical = true,
                RelExpose::Virtual { columns } => {
                    if let Some(vc) = find_virtual_column(columns, column) {
                        candidates.push(vc.sources.clone());
                    }
                }
            }
        }

        if has_physical {
            return Err(LineageError::UnresolvedColumn {
                column: column.to_string(),
                reason: "ambiguous unqualified column across multiple tables".to_string(),
            });
        }
        match candidates.len() {
            1 => Ok(candidates.into_iter().next().unwrap_or_default()),
            0 => Err(LineageError::UnresolvedColumn {
                column: column.to_string(),
                reason: "column not found in any source relation".to_string(),
            }),
            _ => Err(LineageError::UnresolvedColumn {
                column: column.to_string(),
                reason: "ambiguous: column present in multiple source relations".to_string(),
            }),
        }
    }

    /// Expands an unqualified `*` into one entry per visible column.
    ///
    /// # Errors
    /// Fails closed for a physical base table, whose column list is unknown.
    pub(crate) fn expand_wildcard(&self) -> Result<ExpandedColumns, LineageError> {
        let mut out = Vec::new();
        for rel in &self.relations {
            expand_relation(rel, &mut out)?;
        }
        Ok(out)
    }

    /// Expands a qualified `name.*` into the named relation's columns.
    ///
    /// # Errors
    /// Fails closed when the relation is unknown or is a physical base table.
    pub(crate) fn expand_qualified_wildcard(&self, name: &str) -> Result<ExpandedColumns, LineageError> {
        let mut out = Vec::new();
        expand_relation(self.relation_named(name)?, &mut out)?;
        Ok(out)
    }

    /// Finds a visible relation by case-insensitive (ASCII) name, or fails closed.
    fn relation_named(&self, name: &str) -> Result<&Relation, LineageError> {
        self.relations
            .iter()
            .find(|r| r.visible.eq_ignore_ascii_case(name))
            .ok_or_else(|| LineageError::UnknownTable {
                table: name.to_string(),
            })
    }
}

/// Finds a virtual column by case-insensitive (ASCII) output name.
fn find_virtual_column<'a>(columns: &'a [VirtCol], name: &str) -> Option<&'a VirtCol> {
    columns
        .iter()
        .find(|c| c.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

/// Resolves a column within a single known relation.
fn resolve_in_relation(rel: &Relation, column: &str) -> Result<BTreeSet<SourceRef>, LineageError> {
    match &rel.expose {
        RelExpose::Physical { table } => {
            let mut s = BTreeSet::new();
            s.insert(SourceRef::new(table.clone(), column));
            Ok(s)
        }
        RelExpose::Virtual { columns } => find_virtual_column(columns, column)
            .map(|c| c.sources.clone())
            .ok_or_else(|| LineageError::UnresolvedColumn {
                column: column.to_string(),
                reason: format!("not produced by relation `{}`", rel.visible),
            }),
    }
}

/// Appends a relation's columns to a wildcard-expansion accumulator.
fn expand_relation(rel: &Relation, out: &mut ExpandedColumns) -> Result<(), LineageError> {
    match &rel.expose {
        RelExpose::Physical { .. } => Err(LineageError::UnresolvedColumn {
            column: "*".to_string(),
            reason: "wildcard over a base table cannot be expanded (no column list)".to_string(),
        }),
        RelExpose::Virtual { columns } => {
            for vc in columns {
                out.push((vc.name.clone(), vc.sources.clone()));
            }
            Ok(())
        }
    }
}
