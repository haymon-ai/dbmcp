//! Resolves a parsed query into per-output-column source lineage.
//!
//! Walks query scopes (final `SELECT`, CTEs, derived tables, subqueries),
//! resolving each projected expression's column references to physical
//! [`SourceRef`]s. Fails closed on any unresolvable projected column.

use std::collections::{BTreeSet, HashMap};
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, ObjectName, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr, TableFactor, TableWithJoins,
    Values, Visit, Visitor,
};

use crate::error::LineageError;
use crate::model::{OutputColumn, SourceRef, TableRef};
use crate::scope::{ExpandedColumns, RelExpose, Scope, VirtCol};

/// Maximum query-nesting depth before failing with [`LineageError::DepthLimitExceeded`].
const MAX_DEPTH: usize = 64;

/// Map of (folded) CTE name → its defining query.
pub(crate) type CteMap<'a> = HashMap<String, &'a Query>;

/// Set of (folded) CTE names currently being resolved, for cycle detection.
pub(crate) type ActiveCtes = BTreeSet<String>;

/// Lowercases an ASCII string for case-insensitive matching.
fn fold(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Resolves a top-level query with no inherited CTEs.
///
/// # Errors
/// Returns [`LineageError`] on unresolved columns or excessive nesting.
pub(crate) fn resolve(query: &Query) -> Result<Vec<OutputColumn>, LineageError> {
    resolve_query(query, &CteMap::new(), &ActiveCtes::new(), 0)
}

/// Resolves a query into its ordered output columns within a CTE context.
///
/// `active` carries the CTE names currently being resolved; a repeat triggers
/// [`LineageError::CircularReference`] (this also rejects `WITH RECURSIVE`).
///
/// # Errors
/// Returns [`LineageError`] on unresolved columns, cycles, or excessive nesting.
fn resolve_query<'a>(
    query: &'a Query,
    parent: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<Vec<OutputColumn>, LineageError> {
    if depth > MAX_DEPTH {
        return Err(LineageError::DepthLimitExceeded);
    }
    let mut ctes: CteMap<'a> = parent.clone();
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            ctes.insert(fold(&cte.alias.name.value), &cte.query);
        }
    }
    resolve_setexpr(&query.body, &ctes, active, depth)
}

/// Resolves a `SetExpr` (SELECT, set operation, `VALUES`, or parenthesized query).
fn resolve_setexpr<'a>(
    body: &'a SetExpr,
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<Vec<OutputColumn>, LineageError> {
    match body {
        SetExpr::Select(select) => resolve_select(select, ctes, active, depth),
        SetExpr::Query(query) => resolve_query(query, ctes, active, depth + 1),
        SetExpr::SetOperation { left, right, .. } => {
            let left_cols = resolve_setexpr(left, ctes, active, depth)?;
            let right_cols = resolve_setexpr(right, ctes, active, depth)?;
            let mut out = Vec::with_capacity(left_cols.len());
            for (i, lc) in left_cols.into_iter().enumerate() {
                let mut sources = lc.sources;
                if let Some(rc) = right_cols.get(i) {
                    sources.extend(rc.sources.iter().cloned());
                }
                push_column(&mut out, lc.name, sources);
            }
            Ok(out)
        }
        SetExpr::Values(values) => resolve_values(values, ctes, active, depth),
        _ => Ok(Vec::new()),
    }
}

/// Resolves a `VALUES` clause into one unnamed output column per position.
///
/// Each row's expressions are resolved against an empty scope, so literals
/// contribute no sources; a bare column reference fails closed.
fn resolve_values<'a>(
    values: &'a Values,
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<Vec<OutputColumn>, LineageError> {
    let scope = Scope::default();
    let mut out: Vec<OutputColumn> = Vec::new();
    for row in &values.rows {
        for (i, expr) in row.content.iter().enumerate() {
            let sources = collect_sources(expr, &scope, ctes, active, depth)?;
            match out.get_mut(i) {
                Some(col) => col.sources.extend(sources),
                None => push_column(&mut out, None, sources),
            }
        }
    }
    Ok(out)
}

/// Resolves a single `SELECT` into its output columns.
fn resolve_select<'a>(
    select: &'a Select,
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<Vec<OutputColumn>, LineageError> {
    let scope = build_scope(&select.from, ctes, active, depth)?;
    let mut out: Vec<OutputColumn> = Vec::new();

    for item in &select.projection {
        let (expr, name) = match item {
            SelectItem::UnnamedExpr(expr) => (expr, output_name(expr)),
            SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
            SelectItem::ExprWithAliases { expr, aliases } => (expr, aliases.first().map(|a| a.value.clone())),
            SelectItem::Wildcard(_) => {
                push_expanded(&mut out, scope.expand_wildcard()?);
                continue;
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(obj), _) => {
                push_expanded(&mut out, scope.expand_qualified_wildcard(last_ident(obj))?);
                continue;
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => {
                return Err(LineageError::UnresolvedColumn {
                    column: "*".to_string(),
                    reason: "wildcard on an arbitrary expression is unsupported".to_string(),
                });
            }
        };
        let sources = collect_sources(expr, &scope, ctes, active, depth)?;
        push_column(&mut out, name, sources);
    }

    Ok(out)
}

/// Builds the visible-relation scope for a `FROM`/`JOIN` list.
///
/// # Errors
/// Returns [`LineageError`] on unresolved nested queries, cycles, or nesting.
pub(crate) fn build_scope<'a>(
    from: &'a [TableWithJoins],
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<Scope, LineageError> {
    let mut scope = Scope::default();
    for twj in from {
        add_table_with_joins(twj, &mut scope, ctes, active, depth)?;
    }
    Ok(scope)
}

/// Adds a relation and each of its joined relations to a scope.
///
/// # Errors
/// Returns [`LineageError`] on unresolved nested queries, cycles, or nesting.
pub(crate) fn add_table_with_joins<'a>(
    twj: &'a TableWithJoins,
    scope: &mut Scope,
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<(), LineageError> {
    add_table_factor(&twj.relation, scope, ctes, active, depth)?;
    for join in &twj.joins {
        add_table_factor(&join.relation, scope, ctes, active, depth)?;
    }
    Ok(())
}

/// Adds one table factor (and any nested joins) to a scope.
///
/// # Errors
/// Returns [`LineageError`] on unresolved nested queries, cycles, or nesting.
pub(crate) fn add_table_factor<'a>(
    factor: &'a TableFactor,
    scope: &mut Scope,
    ctes: &CteMap<'a>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<(), LineageError> {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let last = last_ident(name);
            let visible = alias
                .as_ref()
                .map_or_else(|| last.to_string(), |a| a.name.value.clone());

            let folded = fold(last);
            if let Some(&cte_query) = ctes.get(folded.as_str()) {
                if active.contains(&folded) {
                    return Err(LineageError::CircularReference { cte: last.to_string() });
                }
                let mut active = active.clone();
                active.insert(folded);
                let cols = resolve_query(cte_query, ctes, &active, depth + 1)?;
                scope.push(visible, virtual_from(cols));
            } else {
                let table = table_ref_from_object_name(name);
                scope.push(visible, RelExpose::Physical { table });
            }
            Ok(())
        }
        TableFactor::Derived { subquery, alias, .. } => {
            let cols = resolve_query(subquery, ctes, active, depth + 1)?;
            let visible = alias.as_ref().map_or_else(String::new, |a| a.name.value.clone());
            scope.push(visible, virtual_from(cols));
            Ok(())
        }
        TableFactor::NestedJoin { table_with_joins, .. } => {
            add_table_with_joins(table_with_joins, scope, ctes, active, depth)
        }
        _ => Ok(()),
    }
}

/// Appends one output column, assigning its position from the current length.
fn push_column(out: &mut Vec<OutputColumn>, name: Option<String>, sources: BTreeSet<SourceRef>) {
    let position = out.len();
    out.push(OutputColumn {
        name,
        position,
        sources,
    });
}

/// Appends wildcard-expanded columns to the projection, assigning positions.
fn push_expanded(out: &mut Vec<OutputColumn>, expanded: ExpandedColumns) {
    for (name, sources) in expanded {
        push_column(out, name, sources);
    }
}

/// Wraps resolved output columns as a virtual relation.
fn virtual_from(cols: Vec<OutputColumn>) -> RelExpose {
    let columns = cols
        .into_iter()
        .map(|c| VirtCol {
            name: c.name,
            sources: c.sources,
        })
        .collect();
    RelExpose::Virtual { columns }
}

/// Returns the last identifier of an object name (its bare name), or `""`.
pub(crate) fn last_ident(name: &ObjectName) -> &str {
    name.0
        .last()
        .and_then(|p| p.as_ident())
        .map_or("", |i| i.value.as_str())
}

/// Builds a [`TableRef`] from an object name, keeping schema/database qualifiers.
///
/// Maps the last three identifier parts to `database.schema.name`; any deeper
/// prefix (e.g. a linked-server qualifier) is deliberately dropped.
pub(crate) fn table_ref_from_object_name(name: &ObjectName) -> TableRef {
    let idents: Vec<&str> = name
        .0
        .iter()
        .filter_map(|p| p.as_ident())
        .map(|i| i.value.as_str())
        .collect();
    match idents.as_slice() {
        [] => TableRef::new(name.to_string()),
        [table] => TableRef::new(*table),
        [schema, table] => TableRef::qualified(*schema, *table),
        [.., database, schema, table] => TableRef::builder(*table)
            .with_schema(*schema)
            .with_database(*database)
            .build(),
    }
}

/// Derives the client-visible output name of an unaliased projection expression.
fn output_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(id) => Some(id.value.clone()),
        Expr::CompoundIdentifier(parts) => parts.last().map(|i| i.value.clone()),
        _ => None,
    }
}

/// Collects the source columns contributing to one expression.
///
/// # Errors
/// Returns [`LineageError`] when a referenced column cannot be resolved.
pub(crate) fn collect_sources(
    expr: &Expr,
    scope: &Scope,
    ctes: &CteMap<'_>,
    active: &ActiveCtes,
    depth: usize,
) -> Result<BTreeSet<SourceRef>, LineageError> {
    let mut collector = ExprCollector {
        scope,
        ctes,
        active,
        depth,
        query_depth: 0,
        sources: BTreeSet::new(),
    };
    match expr.visit(&mut collector) {
        ControlFlow::Break(err) => Err(err),
        ControlFlow::Continue(()) => Ok(collector.sources),
    }
}

/// Visitor that collects column references from an expression subtree.
///
/// A scalar subquery is resolved as a whole (its output sources added); the
/// subquery's inner column references are skipped via `query_depth`.
struct ExprCollector<'a, 's> {
    scope: &'s Scope,
    ctes: &'s CteMap<'a>,
    active: &'s ActiveCtes,
    depth: usize,
    query_depth: usize,
    sources: BTreeSet<SourceRef>,
}

impl Visitor for ExprCollector<'_, '_> {
    type Break = LineageError;

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if self.query_depth == 0 {
            match resolve_query(query, self.ctes, self.active, self.depth + 1) {
                Ok(cols) => {
                    for col in cols {
                        self.sources.extend(col.sources);
                    }
                }
                Err(err) => return ControlFlow::Break(err),
            }
        }
        self.query_depth += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.query_depth = self.query_depth.saturating_sub(1);
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if self.query_depth > 0 {
            return ControlFlow::Continue(());
        }
        let resolved = match expr {
            Expr::Identifier(id) => Some(self.scope.resolve_column(None, &id.value)),
            Expr::CompoundIdentifier(parts) => match parts.as_slice() {
                [.., qualifier, col] => Some(self.scope.resolve_column(Some(&qualifier.value), &col.value)),
                [col] => Some(self.scope.resolve_column(None, &col.value)),
                [] => None,
            },
            _ => None,
        };
        match resolved {
            Some(Ok(found)) => self.sources.extend(found),
            Some(Err(err)) => return ControlFlow::Break(err),
            None => {}
        }
        ControlFlow::Continue(())
    }
}
