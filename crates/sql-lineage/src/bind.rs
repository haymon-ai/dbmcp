//! Name binding over query levels, join trees, CTEs, and output aliases.
//!
//! A [`Level`] is one query or `SELECT`: its CTEs, the [`Tree`]s its `FROM`
//! list exposes, and, once the projection is resolved, its outputs. Lookups
//! that find nothing locally continue in the parent level. A `correlated`
//! child sees the parent's `FROM` relations; a `derived` child (derived
//! table, CTE body, `JOIN … ON`) skips them and continues one level further
//! out. CTEs are visible through every kind of child.
//!
//! A relation the catalog lacks is *open*: its columns are not known yet.
//! Openness propagates through every column list built from it, and a lookup
//! an open relation could satisfy succeeds with no sources instead of
//! failing, so one pass reports every missing name.

use std::collections::BTreeSet;

use sqlparser::ast::{Expr, Ident, TableAlias, Value};

use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{Role, SourceRef};

/// Physical sources of a value.
pub(crate) type Sources = BTreeSet<SourceRef>;

/// A column a relation or `FROM` list exposes.
#[derive(Debug, Clone)]
pub(crate) struct Col {
    /// The column's name, as the server knows it.
    pub name: String,
    /// Physical sources backing the column.
    pub sources: Sources,
    /// Whether `*` expands to the column.
    pub visible: bool,
}

/// A column list that may still be incomplete.
#[derive(Debug, Clone, Default)]
pub(crate) struct Columns {
    /// The known columns, in ordinal order.
    pub cols: Vec<Col>,
    /// Whether columns may be missing because a relation is not in the catalog yet.
    pub open: bool,
}

impl Columns {
    /// Creates a complete column list.
    pub(crate) fn known(cols: Vec<Col>) -> Self {
        Self { cols, open: false }
    }

    /// Creates the empty, open column list of a relation the catalog lacks.
    pub(crate) fn pending() -> Self {
        Self {
            cols: Vec::new(),
            open: true,
        }
    }

    /// Appends another list's columns; the result is open if either is.
    pub(crate) fn extend(&mut self, other: Self) {
        self.cols.extend(other.cols);
        self.open |= other.open;
    }

    /// Unions two set-operation sides position by position.
    ///
    /// # Errors
    /// [`LineageError::ColumnCountMismatch`] when both sides are complete
    /// and differ in length.
    pub(crate) fn union(self, right: Self) -> Result<Self, LineageError> {
        let open = self.open || right.open;
        if !open && self.cols.len() != right.cols.len() {
            return Err(LineageError::ColumnCountMismatch {
                relation: "set operation".to_string(),
                expected: self.cols.len(),
                found: right.cols.len(),
            });
        }
        let cols = self
            .cols
            .into_iter()
            .zip(right.cols)
            .map(|(mut l, r)| {
                l.sources.extend(r.sources);
                l
            })
            .collect();
        Ok(Self { cols, open })
    }

    /// Renames star-visible columns positionally from an alias column list.
    ///
    /// # Errors
    /// [`LineageError::ColumnCountMismatch`] when a complete list is shorter
    /// than the aliases; [`LineageError::Unsupported`] for `AT` or typed
    /// column definitions.
    fn rename(&mut self, dialect: Dialect, alias: &TableAlias) -> Result<(), LineageError> {
        if alias.at.is_some() {
            return Err(unsupported("table alias AT clause"));
        }
        if alias.columns.iter().any(|c| c.data_type.is_some()) {
            return Err(unsupported("column definition list"));
        }
        let visible = self.cols.iter().filter(|c| c.visible).count();
        if !self.open && alias.columns.len() > visible {
            return Err(LineageError::ColumnCountMismatch {
                relation: alias.name.value.clone(),
                expected: visible,
                found: alias.columns.len(),
            });
        }
        for (col, def) in self.cols.iter_mut().filter(|c| c.visible).zip(&alias.columns) {
            col.name = dialect.normalize(&def.name);
        }
        Ok(())
    }

    /// Returns the star-visible columns, keeping openness.
    fn star(&self) -> Self {
        Self {
            cols: self.cols.iter().filter(|c| c.visible).cloned().collect(),
            open: self.open,
        }
    }
}

/// A relation visible under a name.
#[derive(Debug, Clone)]
struct Rel {
    /// The name parts a qualifier may match: the alias, or the written name.
    path: Vec<String>,
    /// The relation's columns.
    columns: Columns,
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

/// How a join tree combines its two sides' columns.
#[derive(Debug)]
pub(crate) enum Keys {
    /// No merge: the columns are concatenated (`ON`, `CROSS JOIN`).
    None,
    /// `USING` keys, merged first.
    Using(Vec<Ident>),
    /// `NATURAL`: every visible column both sides share, merged first.
    Natural,
}

/// One `FROM` item: the relations a qualifier can name and the combined columns.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    rels: Vec<Rel>,
    /// Combined columns for `*` and unqualified lookups, merged keys first.
    cols: Columns,
}

impl Tree {
    /// Builds a tree exposing one relation, renamed by `alias` when given.
    ///
    /// `path` is the qualifier the relation answers to without an alias.
    ///
    /// # Errors
    /// See [`Columns::rename`].
    pub(crate) fn single(
        dialect: Dialect,
        path: Vec<String>,
        mut columns: Columns,
        alias: Option<&TableAlias>,
    ) -> Result<Self, LineageError> {
        let path = match alias {
            Some(alias) => {
                columns.rename(dialect, alias)?;
                vec![dialect.normalize(&alias.name)]
            }
            None => path,
        };
        Ok(Self {
            rels: vec![Rel {
                path,
                columns: columns.clone(),
            }],
            cols: columns,
        })
    }

    /// Wraps the whole tree as one relation, as `(a JOIN b) AS x` does.
    ///
    /// # Errors
    /// See [`Columns::rename`].
    pub(crate) fn aliased(self, dialect: Dialect, alias: &TableAlias) -> Result<Self, LineageError> {
        Self::single(dialect, Vec::new(), self.cols, Some(alias))
    }

    /// Joins `right` onto this tree, returning the merged key sources.
    ///
    /// # Errors
    /// [`LineageError::ColumnNotFound`] or [`LineageError::AmbiguousColumn`]
    /// when a `USING` key is missing from, or repeated in, a complete side.
    pub(crate) fn join(mut self, dialect: Dialect, right: Self, keys: Keys) -> Result<(Self, Sources), LineageError> {
        let keys = match keys {
            Keys::None => Vec::new(),
            Keys::Using(keys) => keys,
            Keys::Natural => right
                .cols
                .cols
                .iter()
                .filter(|r| r.visible)
                // Quoted, so PostgreSQL compares actual names exactly.
                .map(|r| Ident::with_quote('"', r.name.clone()))
                .filter(|key| {
                    self.cols
                        .cols
                        .iter()
                        .any(|l| l.visible && dialect.matches(key, &l.name))
                })
                .collect(),
        };

        let mut left = self.cols;
        let mut right_cols = right.cols;
        let mut merged = Columns {
            cols: Vec::new(),
            open: left.open || right_cols.open,
        };
        let mut joined = Sources::new();
        for key in &keys {
            let l = take(dialect, &mut left, key)?;
            let r = take(dialect, &mut right_cols, key)?;
            let col = match (l, r) {
                (Some(mut l), Some(r)) => {
                    l.sources.extend(r.sources);
                    l.visible = true;
                    l
                }
                (Some(col), None) | (None, Some(col)) => col,
                (None, None) => continue,
            };
            joined.extend(col.sources.iter().cloned());
            merged.cols.push(col);
        }
        merged.cols.extend(left.cols);
        merged.cols.extend(right_cols.cols);
        self.rels.extend(right.rels);
        Ok((
            Self {
                rels: self.rels,
                cols: merged,
            },
            joined,
        ))
    }
}

/// Removes the one column named `key`; `None` if an open list lacks it.
///
/// # Errors
/// [`LineageError::ColumnNotFound`] when a complete list lacks it;
/// [`LineageError::AmbiguousColumn`] when several columns match.
fn take(dialect: Dialect, cols: &mut Columns, key: &Ident) -> Result<Option<Col>, LineageError> {
    let hits: Vec<usize> = cols
        .cols
        .iter()
        .enumerate()
        .filter(|(_, c)| dialect.matches(key, &c.name))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [i] => Ok(Some(cols.cols.remove(*i))),
        [] if cols.open => Ok(None),
        [] => Err(LineageError::ColumnNotFound {
            column: key.value.clone(),
        }),
        _ => Err(LineageError::AmbiguousColumn {
            column: key.value.clone(),
        }),
    }
}

/// The clause an expression sits in, which decides what its names can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clause {
    /// The `SELECT` list, or a `VALUES` row.
    Projection,
    /// `JOIN … ON`.
    On,
    /// `WHERE`.
    Where,
    /// `GROUP BY` or `DISTINCT ON`.
    GroupBy,
    /// `HAVING`.
    Having,
    /// A named `WINDOW` definition.
    Window,
    /// `ORDER BY` of a query.
    OrderBy,
    /// `LIMIT`, `OFFSET`, or `FETCH`.
    Limit,
}

impl Clause {
    /// Returns the role column uses in this clause are recorded with.
    ///
    /// `None` for the projection, whose sources flow into output columns.
    pub(crate) fn role(self) -> Option<Role> {
        match self {
            Self::Projection => None,
            Self::On => Some(Role::Join),
            Self::Where | Self::Having | Self::Limit => Some(Role::Filter),
            Self::GroupBy => Some(Role::GroupBy),
            Self::Window => Some(Role::Window),
            Self::OrderBy => Some(Role::Sort),
        }
    }

    /// Returns whether unqualified names may also bind to output aliases.
    fn sees_outputs(self, dialect: Dialect) -> bool {
        match self {
            Self::GroupBy | Self::Having | Self::OrderBy => true,
            // SQLite lets WHERE fall back to output aliases.
            Self::Where => dialect == Dialect::Sqlite,
            Self::Projection | Self::On | Self::Window | Self::Limit => false,
        }
    }
}

/// The names visible at one query level, chained to the enclosing levels.
#[derive(Debug)]
pub(crate) struct Level<'p> {
    dialect: Dialect,
    parent: Option<&'p Level<'p>>,
    /// Whether lookups leaving this level skip the parent's `FROM` relations.
    derived: bool,
    /// CTEs defined here, in definition order: normalized name and columns.
    ctes: Vec<(String, Columns)>,
    /// What the `FROM` list exposes so far.
    from: Tree,
    /// Output columns, once the projection is resolved.
    outputs: Columns,
}

impl Level<'_> {
    /// Creates the outermost level.
    pub(crate) fn root(dialect: Dialect) -> Self {
        Self {
            dialect,
            parent: None,
            derived: false,
            ctes: Vec::new(),
            from: Tree::default(),
            outputs: Columns::default(),
        }
    }

    fn child(&self, derived: bool) -> Level<'_> {
        Level {
            dialect: self.dialect,
            parent: Some(self),
            derived,
            ctes: Vec::new(),
            from: Tree::default(),
            outputs: Columns::default(),
        }
    }

    /// Creates a child that sees this level's `FROM` relations (expression subqueries).
    pub(crate) fn correlated(&self) -> Level<'_> {
        self.child(false)
    }

    /// Creates a child that skips this level's `FROM` relations (derived tables, CTE bodies, `ON`).
    pub(crate) fn derived(&self) -> Level<'_> {
        self.child(true)
    }

    /// Creates the scope of a `JOIN … ON` condition over `tree`.
    ///
    /// `ON` sees its own join tree and the outer levels, not the `FROM`
    /// items before it. Take the tree back with [`Level::into_tree`].
    pub(crate) fn on(&self, tree: Tree) -> Level<'_> {
        let mut on = self.derived();
        on.add(tree);
        on
    }

    /// Creates the scope of a set operation's `ORDER BY` over its `outputs`.
    ///
    /// The outputs are the only names it sees at this level, both as
    /// columns and as output aliases.
    ///
    /// # Errors
    /// See [`Tree::single`].
    pub(crate) fn sorted(&self, outputs: Columns) -> Result<Level<'_>, LineageError> {
        let mut sorted = self.correlated();
        sorted.add(Tree::single(self.dialect, Vec::new(), outputs.clone(), None)?);
        sorted.set_outputs(outputs);
        Ok(sorted)
    }

    /// Adds a join tree to the `FROM` list.
    pub(crate) fn add(&mut self, tree: Tree) {
        self.from.rels.extend(tree.rels);
        self.from.cols.extend(tree.cols);
    }

    /// Takes the `FROM` list back out as one tree.
    pub(crate) fn into_tree(self) -> Tree {
        self.from
    }

    /// Sets the outputs later clauses may bind to.
    pub(crate) fn set_outputs(&mut self, outputs: Columns) {
        self.outputs = outputs;
    }

    /// Takes the outputs back out.
    pub(crate) fn into_outputs(self) -> Columns {
        self.outputs
    }

    /// Defines a CTE at this level, renamed by its alias column list.
    ///
    /// # Errors
    /// See [`Columns::rename`].
    pub(crate) fn define(&mut self, alias: &TableAlias, mut columns: Columns) -> Result<(), LineageError> {
        columns.rename(self.dialect, alias)?;
        self.ctes.push((self.dialect.normalize(&alias.name), columns));
        Ok(())
    }

    /// Returns a copy of the visible CTE named `name`, innermost first.
    ///
    /// # Errors
    /// [`LineageError::Unsupported`] when `MySQL` would match it only by case.
    pub(crate) fn cte(&self, name: &Ident) -> Result<Option<Columns>, LineageError> {
        let mut level = Some(self);
        while let Some(at) = level {
            for (cte_name, cols) in at.ctes.iter().rev() {
                if self.dialect.matches(name, cte_name) {
                    if self.dialect.strict_cte_case() && name.value != *cte_name {
                        return Err(unsupported("CTE name differing from its definition only by case"));
                    }
                    return Ok(Some(cols.clone()));
                }
            }
            level = at.parent;
        }
        Ok(None)
    }

    /// Expands `*` over this level's `FROM` list.
    ///
    /// # Errors
    /// [`LineageError::ColumnNotFound`] when there is no `FROM` list.
    pub(crate) fn star(&self) -> Result<Columns, LineageError> {
        if self.from.rels.is_empty() {
            return Err(LineageError::ColumnNotFound {
                column: "*".to_string(),
            });
        }
        Ok(self.from.cols.star())
    }

    /// Expands `t.*` over the relation `qualifier` names, searching outer levels.
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when several relations answer;
    /// [`LineageError::ColumnNotFound`] when none does.
    pub(crate) fn star_of(&self, qualifier: &[Ident]) -> Result<Columns, LineageError> {
        for at in self.visible() {
            if let Some(rel) = at.relation_here(qualifier)? {
                return Ok(rel.columns.star());
            }
        }
        Err(LineageError::ColumnNotFound {
            column: format!("{}.*", dotted(qualifier)),
        })
    }

    /// Binds a column reference written in `clause`.
    ///
    /// Searches the `FROM` relations innermost level first; where `clause`
    /// allows it, an unqualified name also binds to this level's outputs,
    /// and a reference matching both takes both sources.
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when more than one candidate binds
    /// at the first level that has any; [`LineageError::ColumnNotFound`] when
    /// no level binds it.
    pub(crate) fn column(&self, clause: Clause, qualifier: &[Ident], name: &Ident) -> Result<Sources, LineageError> {
        let found = self.input(qualifier, name);
        let alias = if qualifier.is_empty() && clause.sees_outputs(self.dialect) {
            self.output_named(name)
        } else {
            None
        };
        match (found, alias) {
            (Ok(mut sources), Some(alias)) => {
                sources.extend(alias);
                Ok(sources)
            }
            (Ok(sources), None) => Ok(sources),
            (Err(LineageError::ColumnNotFound { .. } | LineageError::AmbiguousColumn { .. }), Some(alias)) => Ok(alias),
            (Err(err), _) => Err(err),
        }
    }

    /// Binds a whole `GROUP BY` / `ORDER BY` item that names an output directly.
    ///
    /// A 1-based position selects that output; in `ORDER BY`, a bare name
    /// matching an output sorts by it and does not read input columns.
    /// `None` when the item is an ordinary expression.
    ///
    /// # Errors
    /// [`LineageError::ColumnNotFound`] for a position past a complete output list.
    pub(crate) fn output_item(&self, clause: Clause, expr: &Expr) -> Result<Option<Sources>, LineageError> {
        if clause == Clause::OrderBy
            && let Expr::Identifier(name) = expr
            && let Some(sources) = self.output_named(name)
        {
            return Ok(Some(sources));
        }
        if matches!(clause, Clause::GroupBy | Clause::OrderBy)
            && let Expr::Value(value) = expr
            && let Value::Number(text, _) = &value.value
            && let Ok(position) = text.parse::<usize>()
        {
            return match position.checked_sub(1).and_then(|i| self.outputs.cols.get(i)) {
                Some(col) => Ok(Some(col.sources.clone())),
                None if self.outputs.open => Ok(Some(Sources::new())),
                None => Err(LineageError::ColumnNotFound {
                    column: position.to_string(),
                }),
            };
        }
        Ok(None)
    }

    /// Returns the union of the outputs `name` matches, `None` for no match.
    fn output_named(&self, name: &Ident) -> Option<Sources> {
        let mut hits = self
            .outputs
            .cols
            .iter()
            .filter(|c| self.dialect.matches(name, &c.name))
            .peekable();
        hits.peek()
            .is_some()
            .then(|| hits.flat_map(|c| c.sources.iter().cloned()).collect())
    }

    /// Binds a column reference against `FROM` relations only.
    fn input(&self, qualifier: &[Ident], name: &Ident) -> Result<Sources, LineageError> {
        for at in self.visible() {
            if qualifier.is_empty() {
                if let Some(col) = unique(self.dialect, &at.from.cols.cols, name, qualifier)? {
                    return Ok(col.sources.clone());
                }
                if at.from.cols.open {
                    return Ok(Sources::new());
                }
            } else if let Some(rel) = at.relation_here(qualifier)? {
                return match unique(self.dialect, &rel.columns.cols, name, qualifier)? {
                    Some(col) => Ok(col.sources.clone()),
                    None if rel.columns.open => Ok(Sources::new()),
                    None => Err(not_found(qualifier, name)),
                };
            }
        }
        Err(not_found(qualifier, name))
    }

    /// Returns the levels whose `FROM` relations a lookup here sees, innermost first.
    fn visible(&self) -> impl Iterator<Item = &Level<'_>> {
        let mut next = Some((self, true));
        std::iter::from_fn(move || {
            loop {
                let (at, seen) = next?;
                next = at.parent.map(|parent| (parent, !at.derived));
                if seen {
                    return Some(at);
                }
            }
        })
    }

    /// Finds the relation a qualifier names at this level only.
    fn relation_here(&self, qualifier: &[Ident]) -> Result<Option<&Rel>, LineageError> {
        let mut hits = self.from.rels.iter().filter(|r| r.answers(self.dialect, qualifier));
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

fn unsupported(kind: &str) -> LineageError {
    LineageError::Unsupported(kind.to_string())
}
