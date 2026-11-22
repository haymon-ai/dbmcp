//! Statement tiers: which statements lineage models, and how.
//!
//! A query resolves to its output columns. Relations-only DDL
//! (`CREATE TABLE` without a query, `DROP`, `TRUNCATE`, `CREATE INDEX`)
//! records its target and nothing else. `SHOW` records its [`Show`] form
//! and `DESCRIBE` nothing; `EXPLAIN` carries its [`ExplainForm`] and its
//! inner statement's lineage.
//!
//! DDL that evaluates expressions or reads another relation's rows is
//! rejected: index expressions, partial-index `WHERE`, column `DEFAULT`,
//! `GENERATED`, and other expression options, and `CHECK` and
//! `REFERENCES` constraints, and `MySQL` engines that serve other tables'
//! or servers' rows (`MERGE`, `FEDERATED`, …). Only constraints that name
//! columns pass.
//! Creates and DML are writes. `CREATE TABLE … AS`, `PostgreSQL`
//! `SELECT … INTO`, and `CREATE [MATERIALIZED] VIEW` feed each new column
//! from the query's output; `CREATE TABLE … AS` runs every `CREATE TABLE`
//! check first. `INSERT`, `UPDATE`, `DELETE`, and `MERGE` name the table
//! they write: a `SET` right-hand side or an inserted value flows into its
//! target column, DML `WHERE` and `MERGE … ON` / `WHEN` are filters, and
//! `RETURNING` is the output. Writes through views or derived tables, and
//! every other statement, are [`LineageError::Unsupported`].

use std::ops::ControlFlow;

use sqlparser::ast::{
    Assignment, AssignmentTarget, CascadeOption, ColumnOption, CreateIndex, CreateTable, CreateTableOptions,
    CreateView, Delete, DoUpdate, Expr, FromTable, Ident, IndexColumn, Insert, Merge, MergeAction, MergeInsertKind,
    MergeUpdateKind, ObjectName, ObjectNamePart, ObjectType, OnConflict, OnConflictAction, OnInsert, OrderByExpr,
    Query, SelectItem, SetExpr, SqlOption, Statement, TableAlias, TableAliasColumnDef, TableConstraint, TableFactor,
    TableObject, TableWithJoins, Truncate, Update, UpdateTableFromKind, visit_expressions,
};

use crate::bind::{Clause, Col, Columns, Level, Sources, Tree};
use crate::catalog::Entry;
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::explain::ExplainForm;
use crate::model::{Flow, RelationId, RelationKind, StatementKind, TargetColumn};
use crate::resolve::{Ctx, flow, idents, unsupported};
use crate::show::{Show, computed_filter};

/// What a statement resolves to, before output positions are numbered.
#[derive(Debug)]
pub(crate) struct Analysis {
    /// Output columns: the query's, or the explained statement's.
    pub columns: Columns,
    /// The relation written, created, dropped, truncated, or indexed.
    pub target: Option<RelationId>,
    /// Target columns written, in first-write order.
    pub writes: Vec<TargetColumn>,
    /// The statement kind.
    pub kind: StatementKind,
}

impl Analysis {
    /// Builds an analysis with no output columns.
    fn bare(kind: StatementKind, target: Option<RelationId>) -> Self {
        Self {
            columns: Columns::known(Vec::new()),
            target,
            writes: Vec::new(),
            kind,
        }
    }
}

impl Ctx<'_> {
    /// Resolves one statement of any modeled tier.
    ///
    /// # Errors
    /// [`LineageError::Unsupported`] for statements outside the tiers, and
    /// any resolution error of the statement's parts.
    pub(crate) fn statement(&mut self, statement: &Statement) -> Result<Analysis, LineageError> {
        match statement {
            Statement::Query(query) => self.query_statement(query),
            Statement::CreateTable(create) => self.create_table(create),
            Statement::CreateView(create) => self.create_view(create),
            Statement::Insert(insert) => self.insert(insert),
            Statement::Update(update) => self.update(update),
            Statement::Delete(delete) => self.delete(delete),
            Statement::Merge(merge) => self.merge(merge),
            Statement::CreateIndex(create) => self.create_index(create),
            Statement::Truncate(Truncate {
                table_names,
                if_exists,
                cascade,
                ..
            }) => {
                let [table] = table_names.as_slice() else {
                    return Err(unsupported("TRUNCATE of several tables"));
                };
                if matches!(cascade, Some(CascadeOption::Cascade)) {
                    return Err(unsupported("TRUNCATE CASCADE"));
                }
                let target = self.existing(&table.name, *if_exists)?;
                Ok(Analysis::bare(StatementKind::Truncate, target))
            }
            Statement::Drop {
                object_type,
                if_exists,
                names,
                cascade,
                table,
                ..
            } => {
                if *cascade {
                    return Err(unsupported("DROP CASCADE"));
                }
                let [name] = names.as_slice() else {
                    return Err(unsupported("DROP of several objects"));
                };
                let target = match object_type {
                    ObjectType::Table | ObjectType::View | ObjectType::MaterializedView => {
                        self.existing(name, *if_exists)?
                    }
                    ObjectType::Index => match table {
                        Some(table) => self.existing(table, false)?,
                        None => None,
                    },
                    other => return Err(LineageError::Unsupported(format!("DROP {other}"))),
                };
                Ok(Analysis::bare(StatementKind::Drop, target))
            }
            Statement::ShowVariable { variable } if variable.is_empty() => Err(unsupported("empty SHOW")),
            Statement::ShowVariable { .. }
            | Statement::ShowFunctions { .. }
            | Statement::ShowStatus { .. }
            | Statement::ShowVariables { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowCatalogs { .. }
            | Statement::ShowDatabases { .. }
            | Statement::ShowProcessList { .. }
            | Statement::ShowSchemas { .. }
            | Statement::ShowCharset(_)
            | Statement::ShowObjects(_)
            | Statement::ShowTables { .. }
            | Statement::ShowViews { .. }
            | Statement::ShowCollation { .. } => {
                let tokens = self
                    .dialect
                    .tokenize(self.sql)
                    .map_err(|_| unsupported("SHOW tokens"))?;
                if computed_filter(&tokens) {
                    return Err(unsupported("SHOW filter expression"));
                }
                Ok(Analysis::bare(StatementKind::Show(Show::of(&tokens)), None))
            }
            Statement::ExplainTable { .. } => Ok(Analysis::bare(StatementKind::Describe, None)),
            Statement::Explain {
                statement: explained, ..
            } => {
                let form = ExplainForm::of(self.dialect, statement);
                let mut inner = self.statement(explained)?;
                inner.kind = StatementKind::Explain(form, Box::new(inner.kind));
                Ok(inner)
            }
            other => Err(LineageError::Unsupported(format!("{} statement", variant(other)))),
        }
    }

    /// Resolves a query, or a `PostgreSQL` `SELECT … INTO` as a create.
    ///
    /// `INTO` counts only on the top-level `SELECT`; anywhere else the query
    /// walk rejects it.
    fn query_statement(&mut self, query: &Query) -> Result<Analysis, LineageError> {
        let into = match query.body.as_ref() {
            SetExpr::Select(select) => select.into.as_ref(),
            _ => None,
        };
        let Some(into) = into else {
            let columns = self.query(query, Level::root(self.dialect))?;
            return Ok(Analysis {
                columns,
                ..Analysis::bare(StatementKind::Query, None)
            });
        };
        if self.dialect != Dialect::Postgres {
            return Err(unsupported("SELECT INTO"));
        }
        if into.temporary {
            return Err(unsupported("temporary relation"));
        }
        let name = match into.targets.as_slice() {
            [Expr::Identifier(ident)] => vec![ident.clone()],
            [Expr::CompoundIdentifier(parts)] => parts.clone(),
            _ => return Err(unsupported("SELECT INTO target")),
        };
        let target = self.create_target(&name, RelationKind::Table)?;
        let mut body = query.clone();
        if let SetExpr::Select(select) = body.body.as_mut() {
            select.into = None;
        }
        let cols = self.query(&body, Level::root(self.dialect))?;
        Ok(Analysis {
            writes: created(cols.cols)?,
            ..Analysis::bare(StatementKind::CreateTableAs, target)
        })
    }

    /// Resolves `CREATE TABLE`: the new name is the target, and a query feeds its columns.
    fn create_table(&mut self, create: &CreateTable) -> Result<Analysis, LineageError> {
        if create.query.is_some() && (!create.columns.is_empty() || !create.constraints.is_empty()) {
            // A key built while copying rows leaks values through duplicate-key errors.
            return Err(unsupported("CREATE TABLE AS with column definitions or constraints"));
        }
        if create.like.is_some()
            || create.clone.is_some()
            || create.inherits.is_some()
            || create.partition_of.is_some()
            || create.for_values.is_some()
        {
            return Err(unsupported("CREATE TABLE LIKE/CLONE/INHERITS/PARTITION OF"));
        }
        if create.temporary {
            return Err(unsupported("temporary relation"));
        }
        let options = match &create.table_options {
            CreateTableOptions::None => &[][..],
            CreateTableOptions::With(options)
            | CreateTableOptions::Options(options)
            | CreateTableOptions::Plain(options)
            | CreateTableOptions::TableProperties(options) => options,
        };
        if !options.iter().all(own_rows) {
            return Err(unsupported("table engine serving other rows"));
        }
        if create.primary_key.is_some()
            || create.order_by.is_some()
            || create.partition_by.is_some()
            || create.cluster_by.is_some()
            || create.distkey.is_some()
            || create.sortkey.is_some()
        {
            return Err(unsupported("CREATE TABLE key or partition expression"));
        }
        for column in &create.columns {
            for option in &column.options {
                if !names_only(&option.option) {
                    return Err(unsupported("column option with an expression or reference"));
                }
            }
        }
        for constraint in &create.constraints {
            let keys = match constraint {
                TableConstraint::PrimaryKey(key) => &key.columns,
                TableConstraint::Unique(key) => &key.columns,
                TableConstraint::Index(key) => &key.columns,
                TableConstraint::FulltextOrSpatial(key) => &key.columns,
                _ => return Err(unsupported("table constraint with an expression or reference")),
            };
            bare_columns(keys)?;
        }
        let target = self.create_target(&idents(&create.name)?, RelationKind::Table)?;
        let Some(query) = &create.query else {
            return Ok(Analysis::bare(StatementKind::CreateTable, target));
        };
        let cols = self.query(query, Level::root(self.dialect))?;
        Ok(Analysis {
            writes: created(cols.cols)?,
            ..Analysis::bare(StatementKind::CreateTableAs, target)
        })
    }

    /// Resolves `CREATE [MATERIALIZED] VIEW`: the body feeds the view's columns.
    fn create_view(&mut self, create: &CreateView) -> Result<Analysis, LineageError> {
        if create.or_alter || create.to.is_some() || create.params.is_some() {
            return Err(unsupported("CREATE VIEW clause outside the modeled dialects"));
        }
        if create.temporary {
            return Err(unsupported("temporary relation"));
        }
        if create
            .columns
            .iter()
            .any(|c| c.data_type.is_some() || c.options.is_some())
        {
            return Err(unsupported("view column definition"));
        }
        let (kind, relation) = if create.materialized {
            (StatementKind::CreateMaterializedView, RelationKind::MatView)
        } else {
            (StatementKind::CreateView, RelationKind::View)
        };
        let target = self.create_target(&idents(&create.name)?, relation)?;
        let mut cols = self.query(&create.query, Level::root(self.dialect))?;
        let alias = TableAlias {
            explicit: false,
            name: Ident::new(create.name.to_string()),
            columns: create
                .columns
                .iter()
                .map(|c| TableAliasColumnDef {
                    name: c.name.clone(),
                    data_type: None,
                })
                .collect(),
            at: None,
        };
        cols.rename(self.dialect, &alias)?;
        Ok(Analysis {
            writes: created(cols.cols)?,
            ..Analysis::bare(kind, target)
        })
    }

    /// Resolves `CREATE INDEX` over plain columns: the indexed table is the target.
    fn create_index(&mut self, create: &CreateIndex) -> Result<Analysis, LineageError> {
        if create.predicate.is_some() {
            return Err(unsupported("partial index"));
        }
        bare_columns(&create.columns)?;
        let target = self.existing(&create.table_name, false)?;
        Ok(Analysis::bare(StatementKind::CreateIndex, target))
    }

    /// Resolves `INSERT`: positional target columns, conflict actions, `RETURNING`.
    #[expect(
        clippy::too_many_lines,
        reason = "one destructure over every Insert field, each accepted or rejected"
    )]
    fn insert(&mut self, insert: &Insert) -> Result<Analysis, LineageError> {
        let Insert {
            insert_token: _,
            optimizer_hints: _,
            or: _,
            ignore: _,
            into: _,
            table,
            table_alias,
            columns,
            overwrite,
            source,
            assignments,
            partitioned,
            after_columns,
            has_table_keyword: _,
            on,
            returning,
            output,
            replace_into: _,
            priority: _,
            insert_alias,
            by_name,
            overriding,
            settings,
            format_clause,
            multi_table_insert_type,
            multi_table_into_clauses,
            multi_table_when_clauses,
            multi_table_else_clause,
        } = insert;
        if *overwrite
            || partitioned.is_some()
            || !after_columns.is_empty()
            || output.is_some()
            || insert_alias.is_some()
            || *by_name
            || overriding.is_some()
            || settings.is_some()
            || format_clause.is_some()
            || multi_table_insert_type.is_some()
            || !multi_table_into_clauses.is_empty()
            || !multi_table_when_clauses.is_empty()
            || multi_table_else_clause.is_some()
        {
            return Err(unsupported("INSERT clause outside the modeled dialects"));
        }
        let TableObject::TableName(name) = table else {
            return Err(unsupported("INSERT into a table function"));
        };
        let alias = table_alias.as_ref().map(|a| TableAlias {
            explicit: a.explicit,
            name: a.alias.clone(),
            columns: Vec::new(),
            at: None,
        });
        let tree = self.named(name, alias.as_ref(), &Level::root(self.dialect))?;
        let target = tree.written(self.dialect, &[])?;

        let mut writes = Vec::new();
        let (names, names_open) = self.insert_columns(&tree, columns)?;
        if let Some(source) = source {
            let (values, values_open) = self.insert_source(source)?;
            if !names_open && !values_open && values.len() != names.len() {
                return Err(LineageError::ColumnCountMismatch {
                    relation: name.to_string(),
                    expected: names.len(),
                    found: values.len(),
                });
            }
            for (column, (sources, flow)) in names.iter().zip(values) {
                write(&mut writes, column, sources, flow);
            }
        }
        let level = Level::dml(self.dialect, vec![tree.clone()]);
        for assignment in assignments {
            self.assign(assignment, &tree, &level, &mut None, &mut writes)?;
        }

        match on {
            None => {}
            Some(OnInsert::DuplicateKeyUpdate(assignments)) => {
                for assignment in assignments {
                    if calls_values(&assignment.value) {
                        return Err(unsupported("VALUES() in ON DUPLICATE KEY UPDATE"));
                    }
                    self.assign(assignment, &tree, &level, &mut None, &mut writes)?;
                }
            }
            Some(OnInsert::OnConflict(OnConflict {
                conflict_target: _,
                action,
            })) => match action {
                OnConflictAction::DoNothing => {}
                OnConflictAction::DoUpdate(DoUpdate { assignments, selection }) => {
                    let upsert = Level::upsert(self.dialect, tree.clone(), &writes);
                    for assignment in assignments {
                        self.assign(assignment, &tree, &upsert, &mut None, &mut writes)?;
                    }
                    if let Some(selection) = selection {
                        self.clause(selection, &upsert, Clause::Where)?;
                    }
                }
            },
            Some(_) => return Err(unsupported("INSERT conflict clause")),
        }

        let columns = self.returning(returning.as_deref(), &level)?;
        Ok(Analysis {
            columns,
            writes,
            ..Analysis::bare(StatementKind::Insert, target)
        })
    }

    /// Returns the stored names an `INSERT` column list writes, in order.
    ///
    /// An empty list means every star-visible column in ordinal order. The
    /// flag is set when the target is open, so the names may be incomplete.
    fn insert_columns(&self, tree: &Tree, columns: &[ObjectName]) -> Result<(Vec<String>, bool), LineageError> {
        if columns.is_empty() {
            let star = tree.star();
            return Ok((star.cols.into_iter().map(|c| c.name).collect(), star.open));
        }
        let mut names = Vec::new();
        let mut open = false;
        for column in columns {
            match tree.target(self.dialect, &idents(column)?)? {
                Some((_, name)) => names.push(name),
                None => open = true,
            }
        }
        Ok((names, open))
    }

    /// Resolves an `INSERT` source into per-position sources and flows, plus openness.
    ///
    /// A bare `VALUES` list accepts `DEFAULT`, which has no source; any other
    /// query resolves as usual. Rows merge their flows per position.
    fn insert_source(&mut self, source: &Query) -> Result<(Vec<(Sources, Flow)>, bool), LineageError> {
        let plain = source.with.is_none()
            && source.order_by.is_none()
            && source.limit_clause.is_none()
            && source.fetch.is_none()
            && source.locks.is_empty()
            && source.for_clause.is_none()
            && source.settings.is_none()
            && source.format_clause.is_none()
            && source.pipe_operators.is_empty();
        let (true, SetExpr::Values(values)) = (plain, source.body.as_ref()) else {
            let cols = self.query(source, Level::root(self.dialect))?;
            return Ok((cols.cols.into_iter().map(|c| (c.sources, c.flow)).collect(), cols.open));
        };
        let level = Level::root(self.dialect);
        let mut out: Vec<(Sources, Flow)> = Vec::new();
        for (r, row) in values.rows.iter().enumerate() {
            if r > 0 && row.content.len() != out.len() {
                return Err(LineageError::ColumnCountMismatch {
                    relation: "VALUES".to_string(),
                    expected: out.len(),
                    found: row.content.len(),
                });
            }
            for (i, expr) in row.content.iter().enumerate() {
                let (sources, flow) = self.value(expr, &level)?;
                match out.get_mut(i) {
                    Some((acc, acc_flow)) => {
                        acc.extend(sources);
                        *acc_flow = acc_flow.merge(flow);
                    }
                    None => out.push((sources, flow)),
                }
            }
        }
        Ok((out, false))
    }

    /// Resolves `UPDATE`: `SET` into target columns, `WHERE` as a filter.
    fn update(&mut self, update: &Update) -> Result<Analysis, LineageError> {
        let Update {
            update_token: _,
            optimizer_hints: _,
            table,
            on_cluster,
            assignments,
            from,
            selection,
            returning,
            output,
            or: _,
            order_by,
            limit,
            settings,
        } = update;
        if on_cluster.is_some() || settings.is_some() {
            return Err(unsupported("ON CLUSTER or SETTINGS"));
        }
        if output.is_some() {
            return Err(unsupported("OUTPUT clause"));
        }
        let root = Level::root(self.dialect);
        let targets = self.tree(table, &root, true)?;
        let mut items = vec![targets.clone()];
        match from {
            None => {}
            Some(UpdateTableFromKind::AfterSet(from)) => {
                for (i, item) in from.iter().enumerate() {
                    items.push(self.tree(item, &root, i == 0)?);
                }
            }
            Some(UpdateTableFromKind::BeforeSet(_)) => return Err(unsupported("UPDATE FROM before SET")),
        }
        let level = Level::dml(self.dialect, items);
        let mut target = None;
        let mut writes = Vec::new();
        for assignment in assignments {
            self.assign(assignment, &targets, &level, &mut target, &mut writes)?;
        }
        self.dml_clauses(selection.as_ref(), order_by, limit.as_ref(), &level)?;
        let columns = self.returning(returning.as_deref(), &level)?;
        Ok(Analysis {
            columns,
            writes,
            ..Analysis::bare(StatementKind::Update, target)
        })
    }

    /// Resolves `DELETE`, including `USING` and `MySQL` multi-table forms.
    fn delete(&mut self, delete: &Delete) -> Result<Analysis, LineageError> {
        let Delete {
            delete_token: _,
            optimizer_hints: _,
            tables,
            from,
            on_cluster,
            using,
            selection,
            returning,
            output,
            order_by,
            limit,
            settings,
        } = delete;
        if on_cluster.is_some() || settings.is_some() {
            return Err(unsupported("ON CLUSTER or SETTINGS"));
        }
        if output.is_some() {
            return Err(unsupported("OUTPUT clause"));
        }
        let (FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from)) = from;
        let mysql = matches!(self.dialect, Dialect::Mysql | Dialect::Mariadb);
        // `MySQL` names targets before `FROM`, or in `FROM` ahead of `USING`.
        let (names, items): (Vec<Vec<Ident>>, Vec<&TableWithJoins>) = match (tables.is_empty(), using) {
            (false, None) if mysql => (
                tables.iter().map(idents).collect::<Result<_, _>>()?,
                from.iter().collect(),
            ),
            (true, Some(using)) if mysql => (
                from.iter().map(written_table).collect::<Result<_, _>>()?,
                using.iter().collect(),
            ),
            (true, using) => (
                from.iter().map(written_table).collect::<Result<_, _>>()?,
                from.iter().chain(using.iter().flatten()).collect(),
            ),
            _ => return Err(unsupported("DELETE form")),
        };
        let [name] = names.as_slice() else {
            return Err(unsupported("DELETE from several tables"));
        };
        let root = Level::root(self.dialect);
        let mut trees = Vec::new();
        for (i, item) in items.iter().enumerate() {
            trees.push(self.tree(item, &root, i == 0)?);
        }
        let level = Level::dml(self.dialect, trees);
        let target = level.written(name)?;
        self.dml_clauses(selection.as_ref(), order_by, limit.as_ref(), &level)?;
        let columns = self.returning(returning.as_deref(), &level)?;
        Ok(Analysis {
            columns,
            ..Analysis::bare(StatementKind::Delete, target)
        })
    }

    /// Resolves `MERGE`: `ON` and `WHEN` conditions filter, actions write.
    ///
    /// Every clause sees both the target and the source.
    fn merge(&mut self, merge: &Merge) -> Result<Analysis, LineageError> {
        let Merge {
            merge_token: _,
            optimizer_hints: _,
            into: _,
            table,
            source,
            on,
            clauses,
            output,
        } = merge;
        if output.is_some() {
            return Err(unsupported("MERGE output clause"));
        }
        let root = Level::root(self.dialect);
        let targets = self.factor(table, &root, true)?;
        let source = self.factor(source, &root, true)?;
        let target = targets.written(self.dialect, &[])?;
        let level = Level::dml(self.dialect, vec![targets.clone(), source]);

        self.clause(on, &level, Clause::Where)?;
        let mut writes = Vec::new();
        for clause in clauses {
            if let Some(predicate) = &clause.predicate {
                self.clause(predicate, &level, Clause::Where)?;
            }
            match &clause.action {
                MergeAction::Insert(insert) => {
                    if insert.insert_predicate.is_some() {
                        return Err(unsupported("MERGE INSERT predicate"));
                    }
                    let MergeInsertKind::Values(values) = &insert.kind else {
                        return Err(unsupported("MERGE INSERT ROW or *"));
                    };
                    let [row] = values.rows.as_slice() else {
                        return Err(unsupported("MERGE INSERT of several rows"));
                    };
                    let (names, open) = self.insert_columns(&targets, &insert.columns)?;
                    if !open && names.len() != row.content.len() {
                        return Err(LineageError::ColumnCountMismatch {
                            relation: table.to_string(),
                            expected: names.len(),
                            found: row.content.len(),
                        });
                    }
                    for (column, expr) in names.iter().zip(&row.content) {
                        let (sources, flow) = self.value(expr, &level)?;
                        write(&mut writes, column, sources, flow);
                    }
                }
                MergeAction::Update(update) => {
                    if update.update_predicate.is_some() || update.delete_predicate.is_some() {
                        return Err(unsupported("MERGE UPDATE predicate"));
                    }
                    let MergeUpdateKind::Set(assignments) = &update.kind else {
                        return Err(unsupported("MERGE UPDATE SET *"));
                    };
                    for assignment in assignments {
                        self.assign(assignment, &targets, &level, &mut None, &mut writes)?;
                    }
                }
                MergeAction::Delete { .. } | MergeAction::DoNothing { .. } => {}
            }
        }
        Ok(Analysis {
            writes,
            ..Analysis::bare(StatementKind::Merge, target)
        })
    }

    /// Resolves one written value and its flow; `DEFAULT` has no sources.
    fn value(&mut self, expr: &Expr, level: &Level<'_>) -> Result<(Sources, Flow), LineageError> {
        if is_default(expr) {
            return Ok((Sources::new(), Flow::Default));
        }
        let mut sources = Sources::new();
        self.expr(expr, level, Clause::Projection, &mut sources)?;
        Ok((sources, flow(expr, level)?))
    }

    /// Resolves one `SET` assignment into target-column writes.
    ///
    /// The target column binds among `targets`; the value among `level`. A
    /// tuple target takes a row of values or a subquery, by position.
    /// `target` collects the written table; a second one is rejected.
    fn assign(
        &mut self,
        assignment: &Assignment,
        targets: &Tree,
        level: &Level<'_>,
        target: &mut Option<RelationId>,
        writes: &mut Vec<TargetColumn>,
    ) -> Result<(), LineageError> {
        let (columns, values): (Vec<&ObjectName>, Vec<(Sources, Flow)>) = match (&assignment.target, &assignment.value)
        {
            (AssignmentTarget::ColumnName(column), value) => (vec![column], vec![self.value(value, level)?]),
            (AssignmentTarget::Tuple(columns), Expr::Tuple(values)) if columns.len() == values.len() => {
                let mut all = Vec::new();
                for value in values {
                    all.push(self.value(value, level)?);
                }
                (columns.iter().collect(), all)
            }
            (AssignmentTarget::Tuple(columns), Expr::Subquery(query)) => {
                let cols = self.query(query, level.correlated())?;
                if !cols.open && cols.cols.len() != columns.len() {
                    return Err(LineageError::ColumnCountMismatch {
                        relation: "SET".to_string(),
                        expected: columns.len(),
                        found: cols.cols.len(),
                    });
                }
                (
                    columns.iter().collect(),
                    cols.cols.into_iter().map(|c| (c.sources, c.flow)).collect(),
                )
            }
            (AssignmentTarget::Tuple(_), _) => return Err(unsupported("tuple assignment")),
        };
        for (column, (sources, flow)) in columns.into_iter().zip(values) {
            let Some((id, name)) = targets.target(self.dialect, &idents(column)?)? else {
                continue;
            };
            match target {
                Some(existing) if *existing != id => return Err(unsupported("UPDATE of several tables")),
                Some(_) => {}
                None => *target = Some(id),
            }
            write(writes, &name, sources, flow);
        }
        Ok(())
    }

    /// Records DML `WHERE`, `ORDER BY`, and `LIMIT` uses.
    fn dml_clauses(
        &mut self,
        selection: Option<&Expr>,
        order_by: &[OrderByExpr],
        limit: Option<&Expr>,
        level: &Level<'_>,
    ) -> Result<(), LineageError> {
        if let Some(selection) = selection {
            self.clause(selection, level, Clause::Where)?;
        }
        for item in order_by {
            if item.with_fill.is_some() {
                return Err(unsupported("ORDER BY WITH FILL"));
            }
            self.clause(&item.expr, level, Clause::OrderBy)?;
        }
        if let Some(limit) = limit {
            self.clause(limit, level, Clause::Limit)?;
        }
        Ok(())
    }

    /// Resolves a `RETURNING` list into output columns.
    fn returning(&mut self, returning: Option<&[SelectItem]>, level: &Level<'_>) -> Result<Columns, LineageError> {
        let mut out = Columns::default();
        for item in returning.unwrap_or_default() {
            self.select_item(item, level, &mut out)?;
        }
        Ok(out)
    }

    /// Resolves an existing relation a statement drops, truncates, or indexes.
    ///
    /// With `if_exists`, a vacant name is no target rather than an error; an
    /// absent one still fails, since something the backend cannot describe
    /// exists under it.
    fn existing(&mut self, name: &ObjectName, if_exists: bool) -> Result<Option<RelationId>, LineageError> {
        match self.entry(&idents(name)?) {
            None => Ok(None),
            Some((_, Entry::Vacant(_))) if if_exists => Ok(None),
            Some((written, Entry::Absent | Entry::Vacant(_))) => Err(LineageError::UnknownRelation {
                name: written.to_string(),
            }),
            Some((_, Entry::Resolved(relation))) => {
                self.use_relation(relation);
                Ok(Some(relation.id.clone()))
            }
        }
    }
}

/// Returns whether a column option holds no expression and names no other relation.
fn names_only(option: &ColumnOption) -> bool {
    match option {
        ColumnOption::Null
        | ColumnOption::NotNull
        | ColumnOption::PrimaryKey(_)
        | ColumnOption::Unique(_)
        | ColumnOption::CharacterSet(_)
        | ColumnOption::Collation(_)
        | ColumnOption::Comment(_)
        | ColumnOption::Identity(_)
        | ColumnOption::OnConflict(_)
        | ColumnOption::DialectSpecific(_)
        | ColumnOption::Invisible
        | ColumnOption::Ephemeral(None) => true,
        ColumnOption::Generated { generation_expr, .. } => generation_expr.is_none(),
        _ => false,
    }
}

/// `MySQL` engines that store the table's own rows.
const OWN_ROW_ENGINES: [&str; 5] = ["InnoDB", "MyISAM", "Aria", "MEMORY", "HEAP"];

/// Returns whether a table option keeps the table's rows its own.
///
/// `ENGINE` must be one of [`OWN_ROW_ENGINES`]: `MERGE`, `FEDERATED`,
/// `CONNECT`, `SPIDER`, and the like serve another table's or server's rows.
fn own_rows(option: &SqlOption) -> bool {
    match option {
        SqlOption::NamedParenthesizedList(list) if list.key.value.eq_ignore_ascii_case("ENGINE") => list
            .name
            .as_ref()
            .is_some_and(|engine| OWN_ROW_ENGINES.iter().any(|own| engine.value.eq_ignore_ascii_case(own))),
        _ => true,
    }
}

/// Checks that every index key is a plain column name.
///
/// # Errors
/// [`LineageError::Unsupported`] for an expression key.
fn bare_columns(keys: &[IndexColumn]) -> Result<(), LineageError> {
    for key in keys {
        if !matches!(key.column.expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return Err(unsupported("index expression"));
        }
    }
    Ok(())
}

/// Turns a created relation's query columns into writes, one per column.
///
/// # Errors
/// [`LineageError::Unsupported`] when two columns share a name: `SQLite`
/// keeps both under engine-made names (`name:1`) no rule can match.
fn created(cols: Vec<Col>) -> Result<Vec<TargetColumn>, LineageError> {
    let mut writes: Vec<TargetColumn> = Vec::with_capacity(cols.len());
    for col in cols {
        if writes.iter().any(|w| w.name.to_lowercase() == col.name.to_lowercase()) {
            return Err(unsupported("created columns sharing a name"));
        }
        writes.push(TargetColumn {
            name: col.name,
            sources: col.sources,
            flow: col.flow,
            named: col.named,
        });
    }
    Ok(writes)
}

/// Merges sources and flow into column `name`'s write, appending it when new.
fn write(writes: &mut Vec<TargetColumn>, name: &str, sources: Sources, flow: Flow) {
    match writes.iter_mut().find(|w| w.name == name) {
        Some(existing) => {
            existing.sources.extend(sources);
            existing.flow = existing.flow.merge(flow);
        }
        None => writes.push(TargetColumn {
            name: name.to_string(),
            sources,
            flow,
            named: true,
        }),
    }
}

/// Returns the name a `FROM` item answers to, for a `DELETE` target.
fn written_table(twj: &TableWithJoins) -> Result<Vec<Ident>, LineageError> {
    match (&twj.relation, twj.joins.is_empty()) {
        (
            TableFactor::Table {
                name,
                alias,
                args: None,
                ..
            },
            true,
        ) => match alias {
            Some(alias) => Ok(vec![alias.name.clone()]),
            None => idents(name),
        },
        _ => Err(unsupported("DELETE target")),
    }
}

/// Returns whether an expression calls `VALUES()` or `MariaDB`'s `VALUE()`.
///
/// Both read the value being inserted, which lineage does not model.
fn calls_values(expr: &Expr) -> bool {
    visit_expressions(expr, |e| match e {
        Expr::Function(f)
            if f.name
                .0
                .last()
                .and_then(ObjectNamePart::as_ident)
                .is_some_and(|i| i.value.eq_ignore_ascii_case("values") || i.value.eq_ignore_ascii_case("value")) =>
        {
            ControlFlow::Break(())
        }
        _ => ControlFlow::Continue(()),
    })
    .is_break()
}

/// Returns whether a value is the `DEFAULT` keyword.
pub(crate) fn is_default(expr: &Expr) -> bool {
    matches!(expr, Expr::Identifier(ident) if ident.quote_style.is_none() && ident.value.eq_ignore_ascii_case("default"))
}

/// Returns a statement's variant name, which holds no identifiers or values.
fn variant(statement: &Statement) -> String {
    format!("{statement:?}")
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect()
}
