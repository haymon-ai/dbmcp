//! Resolves queries, `FROM` lists, and joins against the catalog.
//!
//! [`Ctx`] walks a query top-down. Each query level builds a [`Scope`] from
//! its `FROM` list, then resolves the projection into output columns and
//! records every other column use as a [`Reference`] with its [`Role`].
//! Relation names not in the catalog are collected in `missing` so one pass
//! reports them all; while any are missing, errors that may stem from them
//! (unknown or ambiguous columns, count mismatches) are suppressed.

use std::collections::BTreeSet;

use sqlparser::ast::{
    Distinct, Expr, GroupByExpr, GroupByWithModifier, Ident, Join, JoinConstraint, JoinOperator, LimitClause,
    NamedWindowExpr, ObjectName, ObjectNamePart, OrderBy, OrderByExpr, OrderByKind, Query, Select, SelectFlavor,
    SelectItem, SelectItemQualifiedWildcardKind, SetExpr, TableAlias, TableFactor, TableWithJoins, Value, Values,
    WildcardAdditionalOptions,
};

use crate::catalog::{Catalog, Entry, NamePart, WrittenName};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{FunctionUse, Origin, Reference, RelationKind, RelationUse, Role, SourceRef};
use crate::scope::{Col, Rel, Scope};

/// Maximum query-nesting depth before failing with [`LineageError::DepthLimit`].
const MAX_DEPTH: usize = 64;

/// Maximum expression-nesting depth before failing with [`LineageError::DepthLimit`].
///
/// The parser builds long operator chains (`a + b + …`) as left-deep trees
/// without its recursion limit, so the walk bounds its own recursion.
pub(crate) const MAX_EXPR_DEPTH: usize = 256;

/// CTEs visible at one query level, chained to the enclosing levels.
#[derive(Debug)]
pub(crate) struct Ctes<'p> {
    /// Resolved CTEs at this level, in definition order: name and columns.
    defs: Vec<(String, Vec<Col>)>,
    parent: Option<&'p Ctes<'p>>,
}

impl Ctes<'_> {
    /// Creates the empty top-level CTE chain.
    pub(crate) fn root() -> Self {
        Self {
            defs: Vec::new(),
            parent: None,
        }
    }
}

/// What expressions at one position may resolve against.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Walk<'a> {
    pub scope: &'a Scope<'a>,
    pub ctes: &'a Ctes<'a>,
    /// Output columns unqualified names may also bind to, in clauses that allow it.
    pub aliases: Option<&'a [Col]>,
}

/// A `FROM` item's visible relations and combined columns.
#[derive(Debug, Default)]
struct Item {
    /// Relations a qualifier can name.
    rels: Vec<Rel>,
    /// Combined columns, `USING`/`NATURAL` keys merged first.
    cols: Vec<Col>,
}

/// Mutable analysis state shared by the whole statement.
#[derive(Debug)]
pub(crate) struct Ctx<'c> {
    pub dialect: Dialect,
    catalog: &'c Catalog,
    /// Names absent from the snapshot, in discovery order.
    pub missing: Vec<WrittenName>,
    /// Column uses outside the projection.
    pub references: BTreeSet<Reference>,
    /// Catalog relations touched.
    pub relations: BTreeSet<RelationUse>,
    /// Functions called.
    pub functions: BTreeSet<FunctionUse>,
    /// Current query-nesting depth.
    depth: usize,
    /// Current expression-nesting depth.
    pub expr_depth: usize,
}

impl<'c> Ctx<'c> {
    pub(crate) fn new(dialect: Dialect, catalog: &'c Catalog) -> Self {
        Self {
            dialect,
            catalog,
            missing: Vec::new(),
            references: BTreeSet::new(),
            relations: BTreeSet::new(),
            functions: BTreeSet::new(),
            depth: 0,
            expr_depth: 0,
        }
    }

    /// Suppresses errors that may stem from missing relations.
    pub(crate) fn soften<T: Default>(&self, result: Result<T, LineageError>) -> Result<T, LineageError> {
        match result {
            Err(err) if !self.missing.is_empty() && err.is_soft() => Ok(T::default()),
            other => other,
        }
    }

    /// Records each source as a reference with `role`.
    pub(crate) fn refer(&mut self, sources: BTreeSet<SourceRef>, role: Role) {
        for source in sources {
            self.references.insert(Reference {
                source,
                role,
                origin: Origin::Query,
            });
        }
    }

    /// Resolves a query into its output columns.
    ///
    /// # Errors
    /// Returns [`LineageError`] on unresolvable names or unsupported constructs.
    pub(crate) fn query(
        &mut self,
        query: &Query,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
    ) -> Result<Vec<Col>, LineageError> {
        if self.depth >= MAX_DEPTH {
            return Err(LineageError::DepthLimit);
        }
        self.depth += 1;
        let result = self.query_inner(query, outer, ctes);
        self.depth -= 1;
        result
    }

    /// Body of [`Ctx::query`], inside the depth guard.
    fn query_inner(
        &mut self,
        query: &Query,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
    ) -> Result<Vec<Col>, LineageError> {
        let Query {
            with,
            body,
            order_by,
            limit_clause,
            fetch,
            locks: _,
            for_clause,
            settings,
            format_clause,
            pipe_operators,
        } = query;
        if for_clause.is_some() || settings.is_some() || format_clause.is_some() || !pipe_operators.is_empty() {
            return Err(unsupported("query clause (FOR/SETTINGS/FORMAT/pipe)"));
        }

        let mut local = Ctes {
            defs: Vec::new(),
            parent: Some(ctes),
        };
        if let Some(with) = with {
            if with.recursive {
                return Err(unsupported("WITH RECURSIVE"));
            }
            for cte in &with.cte_tables {
                if cte.from.is_some() {
                    return Err(unsupported("CTE FROM clause"));
                }
                let mut cols = self.query(&cte.query, outer, &local)?;
                let renamed = rename(self.dialect, &mut cols, &cte.alias);
                self.soften(renamed)?;
                local.defs.push((self.dialect.normalize(&cte.alias.name), cols));
            }
        }

        let cols = self.set_expr(body, outer, &local, order_by.as_ref())?;

        let bare = Scope::new(outer);
        let walk = Walk {
            scope: &bare,
            ctes: &local,
            aliases: None,
        };
        match limit_clause {
            None => {}
            Some(LimitClause::LimitOffset {
                limit,
                offset,
                limit_by,
            }) => {
                if !limit_by.is_empty() {
                    return Err(unsupported("LIMIT BY"));
                }
                if let Some(limit) = limit {
                    self.clause(limit, walk, Role::Filter)?;
                }
                if let Some(offset) = offset {
                    self.clause(&offset.value, walk, Role::Filter)?;
                }
            }
            Some(LimitClause::OffsetCommaLimit { offset, limit }) => {
                self.clause(offset, walk, Role::Filter)?;
                self.clause(limit, walk, Role::Filter)?;
            }
        }
        if let Some(quantity) = fetch.as_ref().and_then(|f| f.quantity.as_ref()) {
            self.clause(quantity, walk, Role::Filter)?;
        }
        Ok(cols)
    }

    /// Resolves a query body; `order_by` is the enclosing query's.
    fn set_expr(
        &mut self,
        body: &SetExpr,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
        order_by: Option<&OrderBy>,
    ) -> Result<Vec<Col>, LineageError> {
        let cols = match body {
            SetExpr::Select(select) => return self.select(select, outer, ctes, order_by),
            SetExpr::Query(query) => self.query(query, outer, ctes)?,
            SetExpr::SetOperation { left, right, .. } => {
                let left = self.set_expr(left, outer, ctes, None)?;
                let right = self.set_expr(right, outer, ctes, None)?;
                if left.len() != right.len() {
                    return self.soften(Err(LineageError::ColumnCountMismatch {
                        relation: "set operation".to_string(),
                        expected: left.len(),
                        found: right.len(),
                    }));
                }
                left.into_iter()
                    .zip(right)
                    .map(|(mut l, r)| {
                        l.sources.extend(r.sources);
                        l
                    })
                    .collect()
            }
            SetExpr::Values(values) => self.values(values, outer, ctes)?,
            SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => {
                return Err(unsupported("data-modifying statement in a query"));
            }
            SetExpr::Table(_) => return Err(unsupported("TABLE statement")),
        };
        if let Some(order_by) = order_by {
            let scope = Scope {
                rels: Vec::new(),
                cols: cols.clone(),
                parent: outer,
            };
            let walk = Walk {
                scope: &scope,
                ctes,
                aliases: None,
            };
            self.order_by(order_by, walk, &cols)?;
        }
        Ok(cols)
    }

    /// Resolves a `VALUES` list into positional columns.
    fn values(
        &mut self,
        values: &Values,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
    ) -> Result<Vec<Col>, LineageError> {
        let scope = Scope::new(outer);
        let walk = Walk {
            scope: &scope,
            ctes,
            aliases: None,
        };
        let mut cols: Vec<Col> = Vec::new();
        for (r, row) in values.rows.iter().enumerate() {
            if r > 0 && row.content.len() != cols.len() {
                return Err(LineageError::ColumnCountMismatch {
                    relation: "VALUES".to_string(),
                    expected: cols.len(),
                    found: row.content.len(),
                });
            }
            for (i, expr) in row.content.iter().enumerate() {
                let mut sources = BTreeSet::new();
                self.expr(expr, walk, &mut sources)?;
                match cols.get_mut(i) {
                    Some(col) => col.sources.extend(sources),
                    None => cols.push(Col {
                        name: self.dialect.values_column(i),
                        sources,
                        visible: true,
                    }),
                }
            }
        }
        Ok(cols)
    }

    /// Resolves one `SELECT` level.
    fn select(
        &mut self,
        select: &Select,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
        order_by: Option<&OrderBy>,
    ) -> Result<Vec<Col>, LineageError> {
        let Select {
            select_token: _,
            optimizer_hints: _,
            distinct: _,
            select_modifiers: _,
            top,
            top_before_distinct: _,
            projection,
            exclude,
            into,
            from,
            lateral_views,
            prewhere,
            selection: _,
            connect_by,
            group_by: _,
            cluster_by,
            distribute_by,
            sort_by,
            having: _,
            named_window: _,
            qualify,
            window_before_qualify: _,
            value_table_mode,
            flavor,
        } = select;
        if into.is_some() {
            return Err(unsupported("SELECT INTO"));
        }
        if top.is_some()
            || exclude.is_some()
            || !lateral_views.is_empty()
            || prewhere.is_some()
            || !connect_by.is_empty()
            || !cluster_by.is_empty()
            || !distribute_by.is_empty()
            || !sort_by.is_empty()
            || qualify.is_some()
            || value_table_mode.is_some()
            || !matches!(flavor, SelectFlavor::Standard)
        {
            return Err(unsupported("SELECT clause outside the modeled dialects"));
        }

        let mut scope = Scope::new(outer);
        for twj in from {
            let item = self.tree(twj, outer, ctes)?;
            scope.rels.extend(item.rels);
            scope.cols.extend(item.cols);
        }
        let walk = Walk {
            scope: &scope,
            ctes,
            aliases: None,
        };

        let mut out: Vec<Col> = Vec::new();
        for item in projection {
            self.select_item(item, walk, &mut out)?;
        }
        let with_aliases = Walk {
            aliases: Some(&out),
            ..walk
        };

        self.select_clauses(select, walk, with_aliases, &out)?;
        if let Some(order_by) = order_by {
            self.order_by(order_by, with_aliases, &out)?;
        }
        Ok(out)
    }

    /// Records a `SELECT`'s `WHERE`, `GROUP BY`, `HAVING`, `WINDOW`, and `DISTINCT ON` uses.
    fn select_clauses(
        &mut self,
        select: &Select,
        walk: Walk<'_>,
        with_aliases: Walk<'_>,
        out: &[Col],
    ) -> Result<(), LineageError> {
        let Select {
            selection,
            group_by,
            having,
            named_window,
            distinct,
            ..
        } = select;
        if let Some(selection) = selection {
            // SQLite lets WHERE fall back to output aliases.
            let w = if self.dialect == Dialect::Sqlite {
                with_aliases
            } else {
                walk
            };
            self.clause(selection, w, Role::Filter)?;
        }
        match group_by {
            GroupByExpr::All(_) => return Err(unsupported("GROUP BY ALL")),
            GroupByExpr::Expressions(exprs, modifiers) => {
                for expr in exprs {
                    self.positional(expr, with_aliases, out, Role::GroupBy)?;
                }
                for modifier in modifiers {
                    match modifier {
                        GroupByWithModifier::Rollup | GroupByWithModifier::Cube | GroupByWithModifier::Totals => {}
                        GroupByWithModifier::GroupingSets(expr) => self.clause(expr, with_aliases, Role::GroupBy)?,
                    }
                }
            }
        }
        if let Some(having) = having {
            self.clause(having, with_aliases, Role::Filter)?;
        }
        for window in named_window {
            match &window.1 {
                NamedWindowExpr::NamedWindow(_) => {}
                NamedWindowExpr::WindowSpec(spec) => self.window(spec, walk)?,
            }
        }
        match distinct {
            None | Some(Distinct::All | Distinct::Distinct) => {}
            Some(Distinct::On(exprs)) => {
                for expr in exprs {
                    self.positional(expr, with_aliases, out, Role::GroupBy)?;
                }
            }
        }
        Ok(())
    }

    /// Resolves one projection item into output columns.
    fn select_item(&mut self, item: &SelectItem, walk: Walk<'_>, out: &mut Vec<Col>) -> Result<(), LineageError> {
        let (expr, name) = match item {
            SelectItem::UnnamedExpr(expr) => (expr, self.output_name(expr)),
            SelectItem::ExprWithAlias { expr, alias } => (expr, self.dialect.normalize(alias)),
            SelectItem::ExprWithAliases { .. } => return Err(unsupported("multiple aliases on one expression")),
            SelectItem::Wildcard(options) => {
                plain_wildcard(options)?;
                if walk.scope.rels.is_empty() {
                    return Err(LineageError::ColumnNotFound {
                        column: "*".to_string(),
                    });
                }
                out.extend(walk.scope.cols.iter().filter(|c| c.visible).cloned());
                return Ok(());
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(name), options) => {
                plain_wildcard(options)?;
                let qualifier = idents(name)?;
                let cols = walk
                    .scope
                    .relation(self.dialect, &qualifier)
                    .map(|rel| rel.columns.iter().filter(|c| c.visible).cloned().collect::<Vec<_>>());
                out.extend(self.soften(cols)?);
                return Ok(());
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => {
                return Err(unsupported("wildcard on an expression"));
            }
        };
        let mut sources = BTreeSet::new();
        self.expr(expr, walk, &mut sources)?;
        out.push(Col {
            name,
            sources,
            visible: true,
        });
        Ok(())
    }

    /// Returns the informational output name of an unaliased expression.
    fn output_name(&self, expr: &Expr) -> String {
        match expr {
            Expr::Identifier(id) => self.dialect.normalize(id),
            Expr::CompoundIdentifier(parts) => parts.last().map_or_else(String::new, |id| self.dialect.normalize(id)),
            other => other.to_string(),
        }
    }

    /// Records `ORDER BY` items as [`Role::Sort`] references.
    fn order_by(&mut self, order_by: &OrderBy, walk: Walk<'_>, out: &[Col]) -> Result<(), LineageError> {
        if order_by.interpolate.is_some() {
            return Err(unsupported("ORDER BY INTERPOLATE"));
        }
        match &order_by.kind {
            OrderByKind::All(_) => Err(unsupported("ORDER BY ALL")),
            OrderByKind::Expressions(items) => {
                for item in items {
                    self.order_item(item, walk, out)?;
                }
                Ok(())
            }
        }
    }

    fn order_item(&mut self, item: &OrderByExpr, walk: Walk<'_>, out: &[Col]) -> Result<(), LineageError> {
        if item.with_fill.is_some() {
            return Err(unsupported("ORDER BY WITH FILL"));
        }
        // A bare name matching an output column sorts by that column on every
        // modeled dialect; input columns of the same name are not read.
        if let Expr::Identifier(name) = &item.expr {
            let mut hits = out.iter().filter(|c| self.dialect.matches(name, &c.name)).peekable();
            if hits.peek().is_some() {
                let sources = hits.flat_map(|c| c.sources.iter().cloned()).collect();
                self.refer(sources, Role::Sort);
                return Ok(());
            }
        }
        self.positional(&item.expr, walk, out, Role::Sort)
    }

    /// Records a `GROUP BY` / `ORDER BY` item, honoring 1-based positions.
    fn positional(&mut self, expr: &Expr, walk: Walk<'_>, out: &[Col], role: Role) -> Result<(), LineageError> {
        if let Expr::Value(value) = expr
            && let Value::Number(text, _) = &value.value
            && let Ok(position) = text.parse::<usize>()
        {
            let sources = position
                .checked_sub(1)
                .and_then(|i| out.get(i))
                .map(|col| col.sources.clone())
                .ok_or_else(|| LineageError::ColumnNotFound {
                    column: position.to_string(),
                });
            let sources = self.soften(sources)?;
            self.refer(sources, role);
            return Ok(());
        }
        self.clause(expr, walk, role)
    }

    /// Resolves an expression and records its sources with `role`.
    pub(crate) fn clause(&mut self, expr: &Expr, walk: Walk<'_>, role: Role) -> Result<(), LineageError> {
        let mut sources = BTreeSet::new();
        self.expr(expr, walk, &mut sources)?;
        self.refer(sources, role);
        Ok(())
    }

    /// Resolves a `FROM` item and its joins.
    fn tree(&mut self, twj: &TableWithJoins, outer: Option<&Scope<'_>>, ctes: &Ctes<'_>) -> Result<Item, LineageError> {
        let mut item = self.factor(&twj.relation, outer, ctes)?;
        for join in &twj.joins {
            let right = self.factor(&join.relation, outer, ctes)?;
            item = self.join(item, right, join, outer, ctes)?;
        }
        Ok(item)
    }

    /// Joins `right` onto `left`, merging `USING`/`NATURAL` keys.
    fn join(
        &mut self,
        mut left: Item,
        right: Item,
        join: &Join,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
    ) -> Result<Item, LineageError> {
        let (JoinOperator::Join(constraint)
        | JoinOperator::Inner(constraint)
        | JoinOperator::Left(constraint)
        | JoinOperator::LeftOuter(constraint)
        | JoinOperator::Right(constraint)
        | JoinOperator::RightOuter(constraint)
        | JoinOperator::FullOuter(constraint)
        | JoinOperator::CrossJoin(constraint)
        | JoinOperator::StraightJoin(constraint)) = &join.join_operator
        else {
            return Err(unsupported("join kind"));
        };
        let keys: Vec<Ident> = match constraint {
            JoinConstraint::None => Vec::new(),
            JoinConstraint::On(expr) => {
                left.rels.extend(right.rels);
                left.cols.extend(right.cols);
                let scope = Scope {
                    rels: left.rels,
                    cols: left.cols,
                    parent: outer,
                };
                let walk = Walk {
                    scope: &scope,
                    ctes,
                    aliases: None,
                };
                self.clause(expr, walk, Role::Join)?;
                return Ok(Item {
                    rels: scope.rels,
                    cols: scope.cols,
                });
            }
            JoinConstraint::Using(names) => names
                .iter()
                .map(|n| {
                    n.0.last()
                        .and_then(ObjectNamePart::as_ident)
                        .cloned()
                        .ok_or_else(|| unsupported("USING column"))
                })
                .collect::<Result<_, _>>()?,
            JoinConstraint::Natural => right
                .cols
                .iter()
                .filter(|r| r.visible)
                // Quoted, so PostgreSQL compares actual names exactly.
                .map(|r| Ident::with_quote('"', r.name.clone()))
                .filter(|key| {
                    left.cols
                        .iter()
                        .any(|l| l.visible && self.dialect.matches(key, &l.name))
                })
                .collect(),
        };

        let mut merged = Vec::new();
        let mut left_cols = left.cols;
        let mut right_cols = right.cols;
        for key in &keys {
            let l = self.soften(take(self.dialect, &mut left_cols, key))?;
            let r = self.soften(take(self.dialect, &mut right_cols, key))?;
            let (Some(l), Some(r)) = (l, r) else { continue };
            let mut sources = l.sources;
            sources.extend(r.sources);
            self.refer(sources.clone(), Role::Join);
            merged.push(Col {
                name: l.name,
                sources,
                visible: true,
            });
        }
        merged.extend(left_cols);
        merged.extend(right_cols);
        left.rels.extend(right.rels);
        Ok(Item {
            rels: left.rels,
            cols: merged,
        })
    }

    fn factor(
        &mut self,
        factor: &TableFactor,
        outer: Option<&Scope<'_>>,
        ctes: &Ctes<'_>,
    ) -> Result<Item, LineageError> {
        match factor {
            TableFactor::Table {
                name,
                alias,
                args,
                with_hints,
                version,
                with_ordinality,
                partitions: _,
                json_path,
                sample,
                index_hints: _,
            } => {
                if args.is_some() || *with_ordinality {
                    return Err(unsupported("table function in FROM"));
                }
                if !with_hints.is_empty() || version.is_some() || json_path.is_some() || sample.is_some() {
                    return Err(unsupported("table modifier (hints/version/sample)"));
                }
                self.named(name, alias.as_ref(), ctes)
            }
            TableFactor::Derived {
                lateral,
                subquery,
                alias,
                sample,
            } => {
                if *lateral {
                    return Err(unsupported("LATERAL derived table"));
                }
                if sample.is_some() {
                    return Err(unsupported("TABLESAMPLE"));
                }
                let mut cols = self.query(subquery, outer, ctes)?;
                let path = match alias {
                    Some(alias) => {
                        let renamed = rename(self.dialect, &mut cols, alias);
                        self.soften(renamed)?;
                        vec![self.dialect.normalize(&alias.name)]
                    }
                    None => Vec::new(),
                };
                Ok(single(path, cols))
            }
            TableFactor::NestedJoin {
                table_with_joins,
                alias,
            } => {
                let item = self.tree(table_with_joins, outer, ctes)?;
                match alias {
                    None => Ok(item),
                    Some(alias) => {
                        let mut cols = item.cols;
                        let renamed = rename(self.dialect, &mut cols, alias);
                        self.soften(renamed)?;
                        Ok(single(vec![self.dialect.normalize(&alias.name)], cols))
                    }
                }
            }
            TableFactor::TableFunction { .. } | TableFactor::Function { .. } => {
                Err(unsupported("table function in FROM"))
            }
            TableFactor::UNNEST { .. } => Err(unsupported("UNNEST")),
            TableFactor::JsonTable { .. } | TableFactor::OpenJsonTable { .. } => Err(unsupported("JSON_TABLE")),
            TableFactor::Pivot { .. } | TableFactor::Unpivot { .. } | TableFactor::UnpivotExpr { .. } => {
                Err(unsupported("PIVOT/UNPIVOT"))
            }
            TableFactor::MatchRecognize { .. } => Err(unsupported("MATCH_RECOGNIZE")),
            TableFactor::XmlTable { .. } => Err(unsupported("XMLTABLE")),
            TableFactor::SemanticView { .. } => Err(unsupported("SEMANTIC_VIEW")),
        }
    }

    /// Resolves a named relation: a visible CTE, else a catalog entry.
    fn named(&mut self, name: &ObjectName, alias: Option<&TableAlias>, ctes: &Ctes<'_>) -> Result<Item, LineageError> {
        let parts = idents(name)?;
        let path = match alias {
            Some(alias) => vec![self.dialect.normalize(&alias.name)],
            None => parts.iter().map(|p| self.dialect.normalize(p)).collect(),
        };

        if let [single_part] = parts.as_slice()
            && let Some(mut cols) = self.cte(single_part, ctes)?
        {
            if let Some(alias) = alias {
                let renamed = rename(self.dialect, &mut cols, alias);
                self.soften(renamed)?;
            }
            return Ok(single(path, cols));
        }

        let written = WrittenName::query(
            parts
                .iter()
                .map(|p| NamePart {
                    value: p.value.clone(),
                    quoted: p.quote_style.is_some(),
                })
                .collect(),
        );
        let catalog = self.catalog;
        let relation = match catalog.get(&written) {
            None => {
                if !self.missing.contains(&written) {
                    self.missing.push(written);
                }
                return Ok(single(path, Vec::new()));
            }
            Some(Entry::Absent) => {
                return Err(LineageError::UnknownRelation {
                    name: written.to_string(),
                });
            }
            Some(Entry::Resolved(relation)) => relation,
        };
        if relation.kind == RelationKind::View {
            return Err(unsupported("view expansion"));
        }
        self.relations.insert(RelationUse {
            id: relation.id.clone(),
            kind: relation.kind,
            origin: Origin::Query,
        });
        let mut cols: Vec<Col> = relation
            .columns
            .iter()
            .map(|c| Col {
                name: c.name.clone(),
                sources: BTreeSet::from([SourceRef::new(relation.id.clone(), c.name.clone())]),
                visible: c.star_visible,
            })
            .collect();
        if let Some(alias) = alias {
            let renamed = rename(self.dialect, &mut cols, alias);
            self.soften(renamed)?;
        }
        Ok(single(path, cols))
    }

    /// Returns a copy of the visible CTE named `name`, innermost first.
    fn cte(&self, name: &Ident, ctes: &Ctes<'_>) -> Result<Option<Vec<Col>>, LineageError> {
        let mut level = Some(ctes);
        while let Some(chain) = level {
            for (cte_name, cols) in chain.defs.iter().rev() {
                if self.dialect.matches(name, cte_name) {
                    if self.dialect.strict_cte_case() && name.value != *cte_name {
                        return Err(unsupported("CTE name differing from its definition only by case"));
                    }
                    return Ok(Some(cols.clone()));
                }
            }
            level = chain.parent;
        }
        Ok(None)
    }
}

/// Builds an item exposing one relation.
fn single(path: Vec<String>, cols: Vec<Col>) -> Item {
    Item {
        rels: vec![Rel {
            path,
            columns: cols.clone(),
        }],
        cols,
    }
}

/// Removes and returns the one column named `key`.
fn take(dialect: Dialect, cols: &mut Vec<Col>, key: &Ident) -> Result<Option<Col>, LineageError> {
    let hits: Vec<usize> = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| dialect.matches(key, &c.name))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [i] => Ok(Some(cols.remove(*i))),
        [] => Err(LineageError::ColumnNotFound {
            column: key.value.clone(),
        }),
        _ => Err(LineageError::AmbiguousColumn {
            column: key.value.clone(),
        }),
    }
}

/// Renames star-visible columns positionally from an alias column list.
///
/// # Errors
/// [`LineageError::ColumnCountMismatch`] when the list is longer than the
/// columns; [`LineageError::Unsupported`] for typed column definitions.
fn rename(dialect: Dialect, cols: &mut [Col], alias: &TableAlias) -> Result<(), LineageError> {
    if alias.at.is_some() {
        return Err(unsupported("table alias AT clause"));
    }
    if alias.columns.iter().any(|c| c.data_type.is_some()) {
        return Err(unsupported("column definition list"));
    }
    let visible = cols.iter().filter(|c| c.visible).count();
    if alias.columns.len() > visible {
        return Err(LineageError::ColumnCountMismatch {
            relation: alias.name.value.clone(),
            expected: visible,
            found: alias.columns.len(),
        });
    }
    for (col, def) in cols.iter_mut().filter(|c| c.visible).zip(&alias.columns) {
        col.name = dialect.normalize(&def.name);
    }
    Ok(())
}

/// Rejects wildcard modifiers (`EXCLUDE`, `REPLACE`, `RENAME`, …).
fn plain_wildcard(options: &WildcardAdditionalOptions) -> Result<(), LineageError> {
    let WildcardAdditionalOptions {
        wildcard_token: _,
        opt_ilike,
        opt_exclude,
        opt_except,
        opt_replace,
        opt_rename,
        opt_alias,
    } = options;
    if opt_ilike.is_some()
        || opt_exclude.is_some()
        || opt_except.is_some()
        || opt_replace.is_some()
        || opt_rename.is_some()
        || opt_alias.is_some()
    {
        return Err(unsupported("wildcard modifier (EXCLUDE/EXCEPT/REPLACE/RENAME)"));
    }
    Ok(())
}

/// Returns an object name's identifier parts.
///
/// # Errors
/// [`LineageError::Unsupported`] for function-call name parts.
pub(crate) fn idents(name: &ObjectName) -> Result<Vec<Ident>, LineageError> {
    name.0
        .iter()
        .map(|p| p.as_ident().cloned().ok_or_else(|| unsupported("function name part")))
        .collect()
}

pub(crate) fn unsupported(kind: &str) -> LineageError {
    LineageError::Unsupported(kind.to_string())
}
