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
use crate::model::{Flow, RelationId, Role, SourceRef, TargetColumn};
use crate::resolve::unsupported;

/// Physical sources of a value.
pub(crate) type Sources = BTreeSet<SourceRef>;

/// A column a relation or `FROM` list exposes.
#[derive(Debug, Clone)]
pub(crate) struct Col {
    /// The column's name, as the server knows it.
    pub name: String,
    /// Physical sources backing the column.
    pub sources: Sources,
    /// How the value reaches the column from its sources.
    pub flow: Flow,
    /// Whether `*` expands to the column.
    pub visible: bool,
    /// Whether `name` is the name the server keeps (not engine-specific expression text).
    pub named: bool,
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
                l.flow = l.flow.merge(r.flow);
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
    pub(crate) fn rename(&mut self, dialect: Dialect, alias: &TableAlias) -> Result<(), LineageError> {
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
            col.named = true;
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
    /// The catalog table behind the name, which a write may target.
    ///
    /// `None` for views, CTEs, derived tables, aliased joins, and names the
    /// catalog lacks: none of them can be written.
    table: Option<RelationId>,
}

impl Rel {
    /// Returns the table a write into this relation targets.
    ///
    /// `None` while the relation is open (missing from the catalog).
    ///
    /// # Errors
    /// [`LineageError::Unsupported`] for a view or derived relation.
    fn writable(&self) -> Result<Option<RelationId>, LineageError> {
        match &self.table {
            Some(id) => Ok(Some(id.clone())),
            None if self.columns.open => Ok(None),
            None => Err(unsupported("write into a view or derived relation")),
        }
    }

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
    /// `USING` keys, merged; none for `ON` and `CROSS JOIN`, which concatenate.
    Using(Vec<Ident>),
    /// `NATURAL`: every visible column both sides share, merged.
    Natural,
}

/// One `SQLite` `FROM` item a tree holds, with the keys that join it on.
#[derive(Debug, Clone)]
struct Item {
    /// The item's own columns.
    cols: Columns,
    /// The `USING` list, or the `NATURAL` keys in the item's order; none for the first item.
    keys: Vec<Ident>,
}

/// One `FROM` item: the relations a qualifier can name and the combined columns.
#[derive(Debug, Default, Clone)]
pub(crate) struct Tree {
    rels: Vec<Rel>,
    /// Combined columns for `*` and unqualified lookups, in the server's `*` order.
    cols: Columns,
    /// `MySQL`: the columns as a `USING` or `NATURAL` operand, when they differ from `cols`.
    ///
    /// `MySQL` rewrites each `RIGHT JOIN` as a `LEFT JOIN`, which puts its
    /// right side first there; `*` over an `ON` join keeps the written order.
    operand: Option<Columns>,
    /// `SQLite`: the `FROM` items this tree flattens, for [`Tree::nested`].
    items: Vec<Item>,
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
        let items = match dialect {
            Dialect::Sqlite => vec![Item {
                cols: columns.clone(),
                keys: Vec::new(),
            }],
            Dialect::Postgres | Dialect::Mysql | Dialect::Mariadb => Vec::new(),
        };
        Ok(Self {
            rels: vec![Rel {
                path,
                columns: columns.clone(),
                table: None,
            }],
            cols: columns,
            operand: None,
            items,
        })
    }

    /// Marks this single-relation tree as the catalog table `id`, so writes may target it.
    pub(crate) fn with_table(mut self, id: RelationId) -> Self {
        if let [rel] = self.rels.as_mut_slice() {
            rel.table = Some(id);
        }
        self
    }

    /// Returns the table a write names with `qualifier`; empty names the only relation.
    ///
    /// `None` while that relation is open.
    ///
    /// # Errors
    /// [`LineageError::UnknownRelation`] when no relation answers;
    /// [`LineageError::Unsupported`] when several do, or for a view or
    /// derived relation.
    pub(crate) fn written(&self, dialect: Dialect, qualifier: &[Ident]) -> Result<Option<RelationId>, LineageError> {
        let mut hits = self.rels.iter().filter(|r| r.answers(dialect, qualifier));
        match (hits.next(), hits.next()) {
            (Some(rel), None) => rel.writable(),
            (None, _) => Err(LineageError::UnknownRelation {
                name: dotted(qualifier),
            }),
            (Some(_), Some(_)) => Err(unsupported("ambiguous write target")),
        }
    }

    /// Binds a written column (`SET` or `INSERT` list) to its table and catalog name.
    ///
    /// `column` may carry a qualifier naming one of this tree's relations.
    /// The name is the one the catalog stores, even under an alias column
    /// list. `None` while the relation that would hold it is open.
    ///
    /// # Errors
    /// [`LineageError::UnknownRelation`] when the qualifier names no relation;
    /// [`LineageError::ColumnNotFound`] or [`LineageError::AmbiguousColumn`]
    /// when the column binds to no or several columns;
    /// [`LineageError::Unsupported`] for a column of a view or derived relation.
    pub(crate) fn target(
        &self,
        dialect: Dialect,
        column: &[Ident],
    ) -> Result<Option<(RelationId, String)>, LineageError> {
        let Some((name, qualifier)) = column.split_last() else {
            return Err(unsupported("empty column name"));
        };
        let rels: Vec<&Rel> = self
            .rels
            .iter()
            .filter(|r| qualifier.is_empty() || r.answers(dialect, qualifier))
            .collect();
        if rels.is_empty() {
            return Err(LineageError::UnknownRelation {
                name: dotted(qualifier),
            });
        }
        let mut hits = rels.iter().flat_map(|rel| {
            rel.columns
                .cols
                .iter()
                .filter(|c| dialect.matches(name, &c.name))
                .map(move |c| (*rel, c))
        });
        let (rel, col) = match (hits.next(), hits.next()) {
            (Some(hit), None) => hit,
            (None, _) if rels.iter().any(|r| r.columns.open) => return Ok(None),
            (None, _) => return Err(not_found(qualifier, name)),
            (Some(_), Some(_)) => {
                return Err(LineageError::AmbiguousColumn {
                    column: written(qualifier, name),
                });
            }
        };
        let Some(id) = rel.writable()? else {
            return Ok(None);
        };
        let mut sources = col.sources.iter();
        let stored = match (sources.next(), sources.next()) {
            (Some(source), None) => source.column.clone(),
            _ => col.name.clone(),
        };
        Ok(Some((id, stored)))
    }

    /// Returns the star-visible columns, keeping openness.
    pub(crate) fn star(&self) -> Columns {
        self.cols.star()
    }

    /// Wraps the whole tree as one relation, as `(a JOIN b) AS x` does.
    ///
    /// # Errors
    /// See [`Columns::rename`].
    pub(crate) fn aliased(self, dialect: Dialect, alias: &TableAlias) -> Result<Self, LineageError> {
        Self::single(dialect, Vec::new(), self.nested(dialect).cols, Some(alias))
    }

    /// Reads this join as `SQLite` reads a parenthesized join it does not flatten.
    ///
    /// `SQLite` flattens one only as the first item of its `FROM` list,
    /// unaliased; any other is a subquery over its `FROM` items. Its `*`
    /// puts each item's keys before the item to its left and hides the
    /// columns they merge on both items. A repeated name becomes `name:1`,
    /// `name:2`, …, and a column whose name meets an earlier key is hidden
    /// too. Other dialects read the join unchanged.
    pub(crate) fn nested(self, dialect: Dialect) -> Self {
        if dialect != Dialect::Sqlite || self.items.len() < 2 {
            return self;
        }
        // Each term: the column, whether it is a merged key, whether `*` hides it.
        let mut terms = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let next = self.items.get(i + 1).map_or(&[][..], |next| next.keys.as_slice());
            for key in next {
                let mut sources = Sources::new();
                let mut flow = Flow::Null;
                // SQLite binds the key to the leftmost column and the next item's.
                let left = self.items[..=i].iter().find_map(|item| find(dialect, key, &item.cols));
                let right = find(dialect, key, &self.items[i + 1].cols);
                for col in left.into_iter().chain(right) {
                    sources.extend(col.sources.iter().cloned());
                    flow = flow.merge(col.flow);
                }
                let col = Col {
                    name: key.value.clone(),
                    sources,
                    flow,
                    visible: true,
                    named: true,
                };
                terms.push((col, true, false));
            }
            for col in item.cols.cols.iter().filter(|c| c.visible) {
                let merged = item.keys.iter().chain(next).any(|key| dialect.matches(key, &col.name));
                terms.push((col.clone(), false, merged));
            }
        }

        let mut taken: Vec<(String, bool)> = Vec::new();
        let mut cols = Vec::new();
        for (mut col, key, mut hidden) in terms {
            let stem = col.name.trim_end_matches(|c: char| c.is_ascii_digit());
            let stem = stem.strip_suffix(':').unwrap_or(&col.name).to_string();
            let mut suffix = 0;
            while let Some(&(_, earlier)) = taken.iter().find(|(name, _)| name.eq_ignore_ascii_case(&col.name)) {
                hidden |= earlier;
                // Past `:4`, SQLite picks a random suffix.
                if suffix == 4 {
                    col.named = false;
                    break;
                }
                suffix += 1;
                col.name = format!("{stem}:{suffix}");
            }
            if col.named {
                taken.push((col.name.clone(), key));
            }
            if !hidden {
                cols.push(col);
            }
        }
        let cols = Columns {
            cols,
            open: self.cols.open,
        };
        Self {
            rels: self.rels,
            items: vec![Item {
                cols: cols.clone(),
                keys: Vec::new(),
            }],
            cols,
            operand: None,
        }
    }

    /// Appends `right` as a comma or an `ON` join does; `right_join` marks a `RIGHT` join.
    fn append(mut self, dialect: Dialect, right: Self, right_join: bool) -> Self {
        let swapped = dialect == Dialect::Mysql && right_join;
        self.operand = if swapped || self.operand.is_some() || right.operand.is_some() {
            let (mut first, second) = if swapped {
                (right.operand(), self.operand())
            } else {
                (self.operand(), right.operand())
            };
            first.extend(second);
            Some(first)
        } else {
            None
        };
        self.rels.extend(right.rels);
        self.cols.extend(right.cols);
        self.items.extend(right.items);
        self
    }

    /// Returns the columns as a `USING` or `NATURAL` operand.
    fn operand(&self) -> Columns {
        self.operand.clone().unwrap_or_else(|| self.cols.clone())
    }

    /// Joins `right` onto this tree, returning the merged key sources.
    ///
    /// `right_join` marks a `RIGHT` join. The combined columns follow each
    /// server's `*` order, which differs only under `USING` and `NATURAL`:
    ///
    /// - `PostgreSQL` puts the merged keys first, in `USING` order or the
    ///   left side's for `NATURAL`, then the left and the right columns.
    /// - `MySQL` and `MariaDB` put the keys first in the first side's order,
    ///   then that side's columns, then the other's. The first side is the
    ///   right one for a `RIGHT` join. `MySQL` reads each side as its
    ///   operand, and `MariaDB` hides a key the first side hides.
    /// - `SQLite` keeps the left columns in place, each key merged into its
    ///   leftmost match, then appends the right ones. [`Tree::nested`]
    ///   reorders the result as a subquery.
    ///
    /// Every dialect ignores a repeated `USING` key.
    ///
    /// # Errors
    /// [`LineageError::ColumnNotFound`] or [`LineageError::AmbiguousColumn`]
    /// when a `USING` key is missing from, or repeated in, a complete side
    /// (`SQLite`: repeats on the left side merge into the leftmost).
    pub(crate) fn join(
        self,
        dialect: Dialect,
        right: Self,
        keys: Keys,
        right_join: bool,
    ) -> Result<(Self, Sources), LineageError> {
        let keys = match keys {
            Keys::Using(keys) if keys.is_empty() => {
                return Ok((self.append(dialect, right, right_join), Sources::new()));
            }
            // A repeated key merges nothing more.
            Keys::Using(keys) => Keys::Using(keys.into_iter().fold(Vec::new(), |mut unique: Vec<Ident>, key| {
                if !unique.iter().any(|u| dialect.matches(&key, &u.value)) {
                    unique.push(key);
                }
                unique
            })),
            Keys::Natural => Keys::Natural,
        };
        match dialect {
            Dialect::Sqlite => self.join_in_place(dialect, right, keys),
            Dialect::Postgres | Dialect::Mysql | Dialect::Mariadb => {
                self.join_keys_first(dialect, right, keys, right_join)
            }
        }
    }

    /// Joins `right` on `keys` the `SQLite` way; see [`Tree::join`].
    fn join_in_place(mut self, dialect: Dialect, mut right: Self, keys: Keys) -> Result<(Self, Sources), LineageError> {
        let keys = match keys {
            Keys::Using(keys) => keys,
            Keys::Natural => shared(dialect, &right.cols, &self.cols),
        };
        let mut joined = Sources::new();
        let mut extra = Vec::new();
        for key in &keys {
            let at = self.cols.cols.iter().position(|c| dialect.matches(key, &c.name));
            if at.is_none() && !self.cols.open {
                return Err(LineageError::ColumnNotFound {
                    column: key.value.clone(),
                });
            }
            match (at, take(dialect, &mut right.cols, key)?) {
                (Some(i), r) => {
                    let l = &mut self.cols.cols[i];
                    if let Some(r) = r {
                        l.sources.extend(r.sources);
                        l.flow = l.flow.merge(r.flow);
                    }
                    joined.extend(l.sources.iter().cloned());
                }
                (None, Some(r)) => {
                    joined.extend(r.sources.iter().cloned());
                    extra.push(r);
                }
                (None, None) => {}
            }
        }
        if let Some(item) = right.items.first_mut() {
            item.keys = keys;
        }
        self.cols.cols.extend(extra);
        self.cols.extend(right.cols);
        self.rels.extend(right.rels);
        self.items.extend(right.items);
        Ok((self, joined))
    }

    /// Joins `right` on `keys`, merged keys first; see [`Tree::join`].
    fn join_keys_first(
        self,
        dialect: Dialect,
        right: Self,
        keys: Keys,
        right_join: bool,
    ) -> Result<(Self, Sources), LineageError> {
        let left = self.operand.unwrap_or(self.cols);
        let right_cols = right.operand.unwrap_or(right.cols);
        let (mut first, mut second) = if right_join && dialect != Dialect::Postgres {
            (right_cols, left)
        } else {
            (left, right_cols)
        };
        let keys = match keys {
            Keys::Using(keys) if dialect == Dialect::Postgres => keys,
            Keys::Using(mut keys) => {
                keys.sort_by_key(|key| {
                    first
                        .cols
                        .iter()
                        .position(|c| dialect.matches(key, &c.name))
                        .unwrap_or(usize::MAX)
                });
                keys
            }
            Keys::Natural => shared(dialect, &first, &second),
        };

        let open = first.open || second.open;
        let mut joined = Sources::new();
        let mut cols = Vec::new();
        for key in &keys {
            let f = take(dialect, &mut first, key)?;
            let s = take(dialect, &mut second, key)?;
            let col = match (f, s) {
                (Some(mut f), Some(s)) => {
                    f.sources.extend(s.sources);
                    f.flow = f.flow.merge(s.flow);
                    f.visible |= dialect != Dialect::Mariadb;
                    f
                }
                (Some(col), None) | (None, Some(col)) => col,
                (None, None) => continue,
            };
            joined.extend(col.sources.iter().cloned());
            cols.push(col);
        }
        cols.extend(first.cols);
        cols.extend(second.cols);
        let mut rels = self.rels;
        rels.extend(right.rels);
        let mut items = self.items;
        items.extend(right.items);
        let tree = Self {
            rels,
            cols: Columns { cols, open },
            operand: None,
            items,
        };
        Ok((tree, joined))
    }
}

/// Returns the first column named `key`.
fn find<'a>(dialect: Dialect, key: &Ident, cols: &'a Columns) -> Option<&'a Col> {
    cols.cols.iter().find(|c| dialect.matches(key, &c.name))
}

/// Returns the `NATURAL` keys: `from`'s visible columns `other` also shows, in `from`'s order.
fn shared(dialect: Dialect, from: &Columns, other: &Columns) -> Vec<Ident> {
    from.cols
        .iter()
        .filter(|c| c.visible)
        // Quoted, so PostgreSQL compares actual names exactly.
        .map(|c| Ident::with_quote('"', c.name.clone()))
        .filter(|key| other.cols.iter().any(|o| o.visible && dialect.matches(key, &o.name)))
        .collect()
}

/// Removes the one column named `key`; `None` if an open list lacks it.
///
/// # Errors
/// [`LineageError::ColumnNotFound`] when a complete list lacks it;
/// [`LineageError::AmbiguousColumn`] when several columns match.
fn take(dialect: Dialect, cols: &mut Columns, key: &Ident) -> Result<Option<Col>, LineageError> {
    let mut hits = cols
        .cols
        .iter()
        .enumerate()
        .filter(|(_, c)| dialect.matches(key, &c.name))
        .map(|(i, _)| i);
    let hits = (hits.next(), hits.next());
    match hits {
        (Some(i), None) => Ok(Some(cols.cols.remove(i))),
        (None, _) if cols.open => Ok(None),
        (None, _) => Err(LineageError::ColumnNotFound {
            column: key.value.clone(),
        }),
        (Some(_), Some(_)) => Err(LineageError::AmbiguousColumn {
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
            parent: Some(self),
            derived,
            ..Level::root(self.dialect)
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

    /// Creates the scope of a write's values, filters, and `RETURNING`.
    ///
    /// The written table(s) and the `FROM` / `USING` / `MERGE … USING` items
    /// sit side by side as one `FROM` list with no outputs, as in `UPDATE …
    /// FROM`, `DELETE … USING`, `MERGE`, `INSERT … SET` / `ON DUPLICATE KEY
    /// UPDATE`, and every `RETURNING`. Each item was resolved alone, so items
    /// never see each other. For an `INSERT` the list is the target alone;
    /// its source query does not see it.
    ///
    /// A written column read back (`RETURNING`, a later `MySQL` `SET`)
    /// binds to the stored column, not the value written into it. That is
    /// safe only while every write lands in a column ruled like its sources.
    pub(crate) fn dml(dialect: Dialect, items: Vec<Tree>) -> Level<'static> {
        let mut level = Level::root(dialect);
        for item in items {
            level.add(item);
        }
        level
    }

    /// Creates the scope of `ON CONFLICT … DO UPDATE` over the `INSERT` target.
    ///
    /// `SET` values and `WHERE` see the existing row under the target's
    /// name or alias, and the proposed row as `excluded`, qualified only.
    /// Each `excluded` column carries the sources and flow the `INSERT`
    /// writes into it. One it leaves out or writes as `DEFAULT` is
    /// [`Flow::Derived`]: the server fills it, and [`Flow::Default`] would
    /// read as the default of the `SET` target instead.
    pub(crate) fn upsert(dialect: Dialect, target: Tree, writes: &[TargetColumn]) -> Level<'static> {
        let cols = target
            .cols
            .cols
            .iter()
            .map(|c| {
                let written = writes.iter().find(|w| w.name == c.name);
                Col {
                    name: c.name.clone(),
                    sources: written.map(|w| w.sources.clone()).unwrap_or_default(),
                    flow: match written.map(|w| w.flow) {
                        None | Some(Flow::Default) => Flow::Derived,
                        Some(flow) => flow,
                    },
                    visible: c.visible,
                    named: true,
                }
            })
            .collect();
        let excluded = Rel {
            path: vec!["excluded".to_string()],
            columns: Columns {
                cols,
                open: target.cols.open,
            },
            table: None,
        };
        let mut level = Level::dml(dialect, vec![target]);
        level.from.rels.push(excluded);
        level
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
    /// columns and as output aliases. Take them back with [`Level::into_outputs`].
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
        self.from = std::mem::take(&mut self.from).append(self.dialect, tree, false);
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

    /// Returns the table a write names with `qualifier` among this level's `FROM` relations.
    ///
    /// # Errors
    /// See [`Tree::written`].
    pub(crate) fn written(&self, qualifier: &[Ident]) -> Result<Option<RelationId>, LineageError> {
        self.from.written(self.dialect, qualifier)
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
        self.relation(qualifier)?
            .map(|rel| rel.columns.star())
            .ok_or_else(|| LineageError::ColumnNotFound {
                column: format!("{}.*", dotted(qualifier)),
            })
    }

    /// Binds a `PostgreSQL` whole-row reference to the relation `qualifier` names.
    ///
    /// The value carries every column, star-invisible ones included. `None`
    /// when no relation answers or the dialect has no whole-row values.
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when several relations answer.
    pub(crate) fn whole_row(&self, qualifier: &[Ident]) -> Result<Option<Sources>, LineageError> {
        if self.dialect != Dialect::Postgres {
            return Ok(None);
        }
        Ok(self.relation(qualifier)?.map(|rel| {
            rel.columns
                .cols
                .iter()
                .flat_map(|c| c.sources.iter().cloned())
                .collect()
        }))
    }

    /// Finds the relation a qualifier names, innermost level first.
    fn relation(&self, qualifier: &[Ident]) -> Result<Option<&Rel>, LineageError> {
        for at in self.visible() {
            if let Some(rel) = at.relation_here(qualifier)? {
                return Ok(Some(rel));
            }
        }
        Ok(None)
    }

    /// Binds a column reference written in `clause`, with its flow.
    ///
    /// Searches the `FROM` relations innermost level first; where `clause`
    /// allows it, an unqualified name also binds to this level's outputs,
    /// and a reference matching both takes both sources, flows merged. On
    /// `PostgreSQL`, an unqualified name no level's columns bind is a
    /// whole-row reference to the relation it names, if any, which is
    /// [`Flow::Derived`].
    ///
    /// # Errors
    /// [`LineageError::AmbiguousColumn`] when more than one candidate binds
    /// at the first level that has any; [`LineageError::ColumnNotFound`] when
    /// no level binds it.
    pub(crate) fn column(
        &self,
        clause: Clause,
        qualifier: &[Ident],
        name: &Ident,
    ) -> Result<(Sources, Flow), LineageError> {
        let mut found = self.input(qualifier, name);
        if qualifier.is_empty()
            && matches!(found, Err(LineageError::ColumnNotFound { .. }))
            && let Some(whole_row) = self.whole_row(std::slice::from_ref(name)).transpose()
        {
            found = whole_row.map(|sources| (sources, Flow::Derived));
        }
        let alias = if qualifier.is_empty() && clause.sees_outputs(self.dialect) {
            self.output_named(name)
        } else {
            None
        };
        match (found, alias) {
            (Ok((mut sources, flow)), Some((alias, alias_flow))) => {
                sources.extend(alias);
                Ok((sources, flow.merge(alias_flow)))
            }
            (Ok(found), None) => Ok(found),
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
            && let Some((sources, _)) = self.output_named(name)
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

    /// Unions the outputs `name` matches, flows merged; `None` for no match.
    fn output_named(&self, name: &Ident) -> Option<(Sources, Flow)> {
        let mut hits = self
            .outputs
            .cols
            .iter()
            .filter(|c| self.dialect.matches(name, &c.name))
            .peekable();
        let first = hits.peek()?.flow;
        Some(hits.fold((Sources::new(), first), |(mut sources, flow), c| {
            sources.extend(c.sources.iter().cloned());
            (sources, flow.merge(c.flow))
        }))
    }

    /// Binds a column reference against `FROM` relations only.
    ///
    /// A column an open relation could hold binds with no sources and the
    /// [`Flow::Bare`] flow; analysis then fails on the missing name anyway.
    fn input(&self, qualifier: &[Ident], name: &Ident) -> Result<(Sources, Flow), LineageError> {
        for at in self.visible() {
            if qualifier.is_empty() {
                if let Some(col) = unique(self.dialect, &at.from.cols.cols, name, qualifier)? {
                    return Ok((col.sources.clone(), col.flow));
                }
                if at.from.cols.open {
                    return Ok((Sources::new(), Flow::Bare));
                }
            } else if let Some(rel) = at.relation_here(qualifier)? {
                return match unique(self.dialect, &rel.columns.cols, name, qualifier)? {
                    Some(col) => Ok((col.sources.clone(), col.flow)),
                    None if rel.columns.open => Ok((Sources::new(), Flow::Bare)),
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
