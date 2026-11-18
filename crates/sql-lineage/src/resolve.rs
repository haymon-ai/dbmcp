//! Resolves queries, `FROM` lists, and joins against the catalog.
//!
//! [`Ctx`] walks a query top-down. Each query level fills a [`Level`] from
//! its `FROM` list, then resolves the projection into output columns and
//! records every other column use as a [`Reference`] with its [`Role`].
//! Relation names not in the catalog are collected in `missing` so one pass
//! reports them all; each enters its level as an open relation. A view or
//! materialized view expands its body through the same walk, with everything
//! it records tagged [`Origin::View`].

use std::collections::{BTreeMap, BTreeSet};

use sqlparser::ast::{
    Distinct, Expr, GroupByExpr, GroupByWithModifier, Ident, Join, JoinConstraint, JoinOperator, LimitClause,
    NamedWindowExpr, ObjectName, ObjectNamePart, OrderBy, OrderByKind, Query, Select, SelectFlavor, SelectItem,
    SelectItemQualifiedWildcardKind, SetExpr, Statement, TableAlias, TableFactor, TableWithJoins, Values,
    WildcardAdditionalOptions,
};

use crate::bind::{Clause, Col, Columns, Keys, Level, Sources, Tree};
use crate::catalog::{Catalog, Entry, NamePart, Relation, WrittenName};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{
    FunctionUse, Lineage, Origin, OutputColumn, Reference, RelationId, RelationKind, RelationUse, Role, SourceRef,
    StatementKind,
};

/// Maximum query-nesting depth before failing with [`LineageError::DepthLimit`].
const MAX_DEPTH: usize = 64;

/// Maximum expression-nesting depth before failing with [`LineageError::DepthLimit`].
///
/// The parser builds long operator chains (`a + b + …`) as left-deep trees
/// without its recursion limit, so the walk bounds its own recursion.
pub(crate) const MAX_EXPR_DEPTH: usize = 256;

/// Mutable analysis state shared by the whole statement.
#[derive(Debug)]
pub(crate) struct Ctx<'c> {
    pub dialect: Dialect,
    catalog: &'c Catalog,
    /// Names absent from the snapshot, in discovery order.
    missing: Vec<WrittenName>,
    /// Column uses outside the projection.
    references: BTreeSet<Reference>,
    /// Catalog relations touched.
    relations: BTreeSet<RelationUse>,
    /// Functions called.
    functions: BTreeSet<FunctionUse>,
    /// Current query-nesting depth.
    depth: usize,
    /// Current expression-nesting depth.
    pub expr_depth: usize,
    /// Views whose bodies are being expanded, outermost first.
    views: Vec<RelationId>,
    /// Columns of views already expanded in this pass.
    expanded: BTreeMap<RelationId, Vec<Col>>,
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
            views: Vec::new(),
            expanded: BTreeMap::new(),
        }
    }

    /// Returns where the SQL being walked was written.
    fn origin(&self) -> Origin {
        self.views.last().map_or(Origin::Query, |id| Origin::View(id.clone()))
    }

    /// Records a catalog relation named in the SQL being walked.
    fn use_relation(&mut self, relation: &Relation) {
        self.relations.insert(RelationUse {
            id: relation.id.clone(),
            kind: relation.kind,
            origin: self.origin(),
        });
    }

    /// Records a function called in the SQL being walked.
    pub(crate) fn call(&mut self, name: String, known: bool) {
        self.functions.insert(FunctionUse {
            name,
            known,
            origin: self.origin(),
        });
    }

    /// Ends the pass: the lineage of `cols`, unless a name was missing.
    ///
    /// # Errors
    /// Returns [`LineageError::Missing`] with every name absent from the snapshot.
    pub(crate) fn finish(self, cols: Columns) -> Result<Lineage, LineageError> {
        if !self.missing.is_empty() {
            return Err(LineageError::Missing(self.missing));
        }
        Ok(Lineage {
            columns: cols
                .cols
                .into_iter()
                .enumerate()
                .map(|(position, col)| OutputColumn {
                    name: col.name,
                    position,
                    sources: col.sources,
                })
                .collect(),
            references: self.references,
            relations: self.relations,
            functions: self.functions,
            target: None,
            statement: StatementKind::Query,
        })
    }

    /// Records each source as a reference with `role`.
    pub(crate) fn refer(&mut self, sources: Sources, role: Role) {
        for source in sources {
            self.references.insert(Reference {
                source,
                role,
                origin: self.origin(),
            });
        }
    }

    /// Resolves a query at `level`, a fresh child of the enclosing level.
    ///
    /// # Errors
    /// Returns [`LineageError`] on unresolvable names or unsupported constructs.
    pub(crate) fn query(&mut self, query: &Query, level: Level<'_>) -> Result<Columns, LineageError> {
        if self.depth >= MAX_DEPTH {
            return Err(LineageError::DepthLimit);
        }
        self.depth += 1;
        let result = self.query_inner(query, level);
        self.depth -= 1;
        result
    }

    /// Body of [`Ctx::query`], inside the depth guard.
    fn query_inner(&mut self, query: &Query, mut level: Level<'_>) -> Result<Columns, LineageError> {
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

        if let Some(with) = with {
            if with.recursive {
                return Err(unsupported("WITH RECURSIVE"));
            }
            for cte in &with.cte_tables {
                if cte.from.is_some() {
                    return Err(unsupported("CTE FROM clause"));
                }
                let cols = self.query(&cte.query, level.derived())?;
                level.define(&cte.alias, cols)?;
            }
        }

        let cols = self.set_expr(body, &level, order_by.as_ref())?;

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
                    self.clause(limit, &level, Clause::Limit)?;
                }
                if let Some(offset) = offset {
                    self.clause(&offset.value, &level, Clause::Limit)?;
                }
            }
            Some(LimitClause::OffsetCommaLimit { offset, limit }) => {
                self.clause(offset, &level, Clause::Limit)?;
                self.clause(limit, &level, Clause::Limit)?;
            }
        }
        if let Some(quantity) = fetch.as_ref().and_then(|f| f.quantity.as_ref()) {
            self.clause(quantity, &level, Clause::Limit)?;
        }
        Ok(cols)
    }

    /// Resolves a query body; `order_by` is the enclosing query's.
    fn set_expr(
        &mut self,
        body: &SetExpr,
        level: &Level<'_>,
        order_by: Option<&OrderBy>,
    ) -> Result<Columns, LineageError> {
        let mut cols = match body {
            SetExpr::Select(select) => return self.select(select, level.correlated(), order_by),
            SetExpr::Query(query) => self.query(query, level.correlated())?,
            SetExpr::SetOperation { .. } => {
                // `a UNION b UNION c …` nests leftward once per operand, so walk
                // the left spine in a loop; right operands nest only by precedence.
                let mut rights = Vec::new();
                let mut leftmost = body;
                while let SetExpr::SetOperation { left, right, .. } = leftmost {
                    rights.push(right);
                    leftmost = left;
                }
                let mut cols = self.set_expr(leftmost, level, None)?;
                for right in rights.into_iter().rev() {
                    cols = cols.union(self.set_expr(right, level, None)?)?;
                }
                cols
            }
            SetExpr::Values(values) => self.values(values, level)?,
            SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => {
                return Err(unsupported("data-modifying statement in a query"));
            }
            SetExpr::Table(_) => return Err(unsupported("TABLE statement")),
        };
        if let Some(order_by) = order_by {
            let sorted = level.sorted(cols)?;
            self.order_by(order_by, &sorted)?;
            cols = sorted.into_outputs();
        }
        Ok(cols)
    }

    /// Resolves a `VALUES` list into positional columns.
    fn values(&mut self, values: &Values, level: &Level<'_>) -> Result<Columns, LineageError> {
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
                self.expr(expr, level, Clause::Projection, &mut sources)?;
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
        Ok(Columns::known(cols))
    }

    /// Resolves one `SELECT` at `level`, a fresh child of its query's level.
    fn select(
        &mut self,
        select: &Select,
        mut level: Level<'_>,
        order_by: Option<&OrderBy>,
    ) -> Result<Columns, LineageError> {
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

        for twj in from {
            let tree = self.tree(twj, &level)?;
            level.add(tree);
        }

        let mut out = Columns::default();
        for item in projection {
            self.select_item(item, &level, &mut out)?;
        }
        level.set_outputs(out);

        self.select_clauses(select, &level)?;
        if let Some(order_by) = order_by {
            self.order_by(order_by, &level)?;
        }
        Ok(level.into_outputs())
    }

    /// Records a `SELECT`'s `WHERE`, `GROUP BY`, `HAVING`, `WINDOW`, and `DISTINCT ON` uses.
    fn select_clauses(&mut self, select: &Select, level: &Level<'_>) -> Result<(), LineageError> {
        let Select {
            selection,
            group_by,
            having,
            named_window,
            distinct,
            ..
        } = select;
        if let Some(selection) = selection {
            self.clause(selection, level, Clause::Where)?;
        }
        match group_by {
            GroupByExpr::All(_) => return Err(unsupported("GROUP BY ALL")),
            GroupByExpr::Expressions(exprs, modifiers) => {
                for expr in exprs {
                    self.item(expr, level, Clause::GroupBy)?;
                }
                for modifier in modifiers {
                    match modifier {
                        GroupByWithModifier::Rollup | GroupByWithModifier::Cube | GroupByWithModifier::Totals => {}
                        GroupByWithModifier::GroupingSets(expr) => self.clause(expr, level, Clause::GroupBy)?,
                    }
                }
            }
        }
        if let Some(having) = having {
            self.clause(having, level, Clause::Having)?;
        }
        for window in named_window {
            match &window.1 {
                NamedWindowExpr::NamedWindow(_) => {}
                NamedWindowExpr::WindowSpec(spec) => self.window(spec, level, Clause::Window)?,
            }
        }
        match distinct {
            None | Some(Distinct::All | Distinct::Distinct) => {}
            Some(Distinct::On(exprs)) => {
                for expr in exprs {
                    self.item(expr, level, Clause::GroupBy)?;
                }
            }
        }
        Ok(())
    }

    /// Resolves one projection item into output columns.
    fn select_item(&mut self, item: &SelectItem, level: &Level<'_>, out: &mut Columns) -> Result<(), LineageError> {
        let (expr, name) = match item {
            SelectItem::UnnamedExpr(expr) => (expr, self.output_name(expr)),
            SelectItem::ExprWithAlias { expr, alias } => (expr, self.dialect.normalize(alias)),
            SelectItem::ExprWithAliases { .. } => return Err(unsupported("multiple aliases on one expression")),
            SelectItem::Wildcard(options) => {
                plain_wildcard(options)?;
                out.extend(level.star()?);
                return Ok(());
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(name), options) => {
                plain_wildcard(options)?;
                out.extend(level.star_of(&idents(name)?)?);
                return Ok(());
            }
            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => {
                return Err(unsupported("wildcard on an expression"));
            }
        };
        let mut sources = BTreeSet::new();
        self.expr(expr, level, Clause::Projection, &mut sources)?;
        out.cols.push(Col {
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
    fn order_by(&mut self, order_by: &OrderBy, level: &Level<'_>) -> Result<(), LineageError> {
        if order_by.interpolate.is_some() {
            return Err(unsupported("ORDER BY INTERPOLATE"));
        }
        match &order_by.kind {
            OrderByKind::All(_) => Err(unsupported("ORDER BY ALL")),
            OrderByKind::Expressions(items) => {
                for item in items {
                    if item.with_fill.is_some() {
                        return Err(unsupported("ORDER BY WITH FILL"));
                    }
                    self.item(&item.expr, level, Clause::OrderBy)?;
                }
                Ok(())
            }
        }
    }

    /// Records a `GROUP BY` / `ORDER BY` item, which may name an output directly.
    fn item(&mut self, expr: &Expr, level: &Level<'_>, clause: Clause) -> Result<(), LineageError> {
        match level.output_item(clause, expr)? {
            Some(sources) => {
                if let Some(role) = clause.role() {
                    self.refer(sources, role);
                }
                Ok(())
            }
            None => self.clause(expr, level, clause),
        }
    }

    /// Resolves an expression and records its sources with the clause's role.
    pub(crate) fn clause(&mut self, expr: &Expr, level: &Level<'_>, clause: Clause) -> Result<(), LineageError> {
        let mut sources = BTreeSet::new();
        self.expr(expr, level, clause, &mut sources)?;
        if let Some(role) = clause.role() {
            self.refer(sources, role);
        }
        Ok(())
    }

    /// Resolves a `FROM` item and its joins; `level` is the `SELECT` it belongs to.
    fn tree(&mut self, twj: &TableWithJoins, level: &Level<'_>) -> Result<Tree, LineageError> {
        let mut tree = self.factor(&twj.relation, level)?;
        for join in &twj.joins {
            let right = self.factor(&join.relation, level)?;
            tree = self.join(tree, right, join, level)?;
        }
        Ok(tree)
    }

    /// Joins `right` onto `left`, merging `USING`/`NATURAL` keys.
    fn join(&mut self, left: Tree, right: Tree, join: &Join, level: &Level<'_>) -> Result<Tree, LineageError> {
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
        let keys = match constraint {
            JoinConstraint::None | JoinConstraint::On(_) => Keys::Using(Vec::new()),
            JoinConstraint::Using(names) => Keys::Using(
                names
                    .iter()
                    .map(|n| {
                        n.0.last()
                            .and_then(ObjectNamePart::as_ident)
                            .cloned()
                            .ok_or_else(|| unsupported("USING column"))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            JoinConstraint::Natural => Keys::Natural,
        };
        let (tree, joined) = left.join(self.dialect, right, keys)?;
        self.refer(joined, Role::Join);
        let JoinConstraint::On(expr) = constraint else {
            return Ok(tree);
        };
        let on = level.on(tree);
        self.clause(expr, &on, Clause::On)?;
        Ok(on.into_tree())
    }

    fn factor(&mut self, factor: &TableFactor, level: &Level<'_>) -> Result<Tree, LineageError> {
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
                self.named(name, alias.as_ref(), level)
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
                let cols = self.query(subquery, level.derived())?;
                Tree::single(self.dialect, Vec::new(), cols, alias.as_ref())
            }
            TableFactor::NestedJoin {
                table_with_joins,
                alias,
            } => {
                let tree = self.tree(table_with_joins, level)?;
                match alias {
                    None => Ok(tree),
                    Some(alias) => tree.aliased(self.dialect, alias),
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
    fn named(
        &mut self,
        name: &ObjectName,
        alias: Option<&TableAlias>,
        level: &Level<'_>,
    ) -> Result<Tree, LineageError> {
        let parts = idents(name)?;
        let path: Vec<String> = parts.iter().map(|p| self.dialect.normalize(p)).collect();

        if let [single_part] = parts.as_slice()
            && let Some(cols) = level.cte(single_part)?
        {
            return Tree::single(self.dialect, path, cols, alias);
        }

        let written = WrittenName {
            parts: parts
                .iter()
                .map(|p| NamePart {
                    value: p.value.clone(),
                    quoted: p.quote_style.is_some(),
                })
                .collect(),
            context: self.origin(),
        };
        let catalog = self.catalog;
        let relation = match catalog.get(&written) {
            None => {
                if !self.missing.contains(&written) {
                    self.missing.push(written);
                }
                return Tree::single(self.dialect, path, Columns::pending(), alias);
            }
            Some(Entry::Absent) => {
                return Err(LineageError::UnknownRelation {
                    name: written.to_string(),
                });
            }
            Some(Entry::Resolved(relation)) => relation,
        };
        self.use_relation(relation);
        let cols = match relation.kind {
            RelationKind::View | RelationKind::MatView => self.view(relation)?,
            _ => relation
                .columns
                .iter()
                .map(|c| Col {
                    name: c.name.clone(),
                    sources: BTreeSet::from([SourceRef::new(relation.id.clone(), c.name.clone())]),
                    visible: c.star_visible,
                })
                .collect(),
        };
        Tree::single(self.dialect, path, Columns::known(cols), alias)
    }

    /// Expands a view or materialized view body into the view's columns.
    ///
    /// The body resolves without the query's scopes or CTEs. Its output
    /// columns take the catalog's names and visibility by position; an open
    /// body yields them without sources. Each view expands once per pass.
    ///
    /// # Errors
    /// [`LineageError::Unsupported`] without a body or for a body that is not
    /// one query; [`LineageError::CircularReference`] when the view is
    /// already being expanded; [`LineageError::Parse`] for an unparsable
    /// body; [`LineageError::ColumnCountMismatch`] when a complete body's
    /// column count differs from the catalog's.
    fn view(&mut self, relation: &Relation) -> Result<Vec<Col>, LineageError> {
        if let Some(cols) = self.expanded.get(&relation.id) {
            return Ok(cols.clone());
        }
        let Some(sql) = relation.view_sql.as_deref() else {
            return Err(unsupported("view without a body"));
        };
        if self.views.contains(&relation.id) {
            return Err(LineageError::CircularReference {
                name: relation.id.name.clone(),
            });
        }
        let statements = self
            .dialect
            .parse(sql)
            .map_err(|e| LineageError::Parse(format!("view `{}`: {e}", relation.id.name)))?;
        let [Statement::Query(query)] = statements.as_slice() else {
            return Err(unsupported("view body other than one query"));
        };

        self.views.push(relation.id.clone());
        let body = self.query(query, Level::root(self.dialect));
        self.views.pop();
        let body = body?;

        if body.open {
            let cols: Vec<Col> = relation
                .columns
                .iter()
                .map(|c| Col {
                    name: c.name.clone(),
                    sources: Sources::new(),
                    visible: c.star_visible,
                })
                .collect();
            self.expanded.insert(relation.id.clone(), cols.clone());
            return Ok(cols);
        }
        if body.cols.len() != relation.columns.len() {
            return Err(LineageError::ColumnCountMismatch {
                relation: relation.id.name.clone(),
                expected: relation.columns.len(),
                found: body.cols.len(),
            });
        }
        let cols: Vec<Col> = relation
            .columns
            .iter()
            .zip(body.cols)
            .map(|(c, b)| Col {
                name: c.name.clone(),
                sources: b.sources,
                visible: c.star_visible,
            })
            .collect();
        self.expanded.insert(relation.id.clone(), cols.clone());
        Ok(cols)
    }
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
