//! Statement tiers: which statements lineage models, and how.
//!
//! A query resolves to its output columns. Relations-only DDL
//! (`CREATE TABLE` without a query, `DROP`, `TRUNCATE`, `CREATE INDEX`)
//! records its target and nothing else. `SHOW` and `DESCRIBE` record
//! nothing; `EXPLAIN` carries its inner statement's lineage.
//!
//! DDL that evaluates expressions or reads another relation's rows is
//! rejected: index expressions, partial-index `WHERE`, column `DEFAULT`,
//! `GENERATED`, and other expression options, and `CHECK` and
//! `REFERENCES` constraints, and `MySQL` engines that serve other tables'
//! or servers' rows (`MERGE`, `FEDERATED`, …). Only constraints that name
//! columns pass.
//! Creates are writes: `CREATE TABLE … AS`, `PostgreSQL` `SELECT … INTO`,
//! and `CREATE [MATERIALIZED] VIEW` name their target and feed each new
//! column from the query's output. `CREATE TABLE … AS` runs every
//! `CREATE TABLE` check first. Every other statement,
//! `INSERT`/`UPDATE`/`DELETE`/`MERGE` included, is
//! [`LineageError::Unsupported`].

use sqlparser::ast::{
    CascadeOption, ColumnOption, CreateIndex, CreateTable, CreateTableOptions, CreateView, Expr, Ident, IndexColumn,
    ObjectName, ObjectType, Query, SetExpr, SqlOption, Statement, TableAlias, TableAliasColumnDef, TableConstraint,
    Truncate,
};

use crate::bind::{Col, Columns, Level};
use crate::catalog::Entry;
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{RelationId, RelationKind, StatementKind, TargetColumn};
use crate::resolve::{Ctx, idents, unsupported};

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
            Statement::ShowVariable { variable } => {
                let first = variable.first().ok_or_else(|| unsupported("empty SHOW"))?;
                Ok(Analysis::bare(
                    StatementKind::Show(first.value.to_ascii_uppercase()),
                    None,
                ))
            }
            Statement::ShowFunctions { .. } => Ok(show("FUNCTIONS")),
            Statement::ShowStatus { .. } => Ok(show("STATUS")),
            Statement::ShowVariables { .. } => Ok(show("VARIABLES")),
            Statement::ShowCreate { .. } => Ok(show("CREATE")),
            Statement::ShowColumns { .. } => Ok(show("COLUMNS")),
            Statement::ShowCatalogs { .. } => Ok(show("CATALOGS")),
            Statement::ShowDatabases { .. } => Ok(show("DATABASES")),
            Statement::ShowProcessList { .. } => Ok(show("PROCESSLIST")),
            Statement::ShowSchemas { .. } => Ok(show("SCHEMAS")),
            Statement::ShowCharset(_) => Ok(show("CHARSET")),
            Statement::ShowObjects(_) => Ok(show("OBJECTS")),
            Statement::ShowTables { .. } => Ok(show("TABLES")),
            Statement::ShowViews { .. } => Ok(show("VIEWS")),
            Statement::ShowCollation { .. } => Ok(show("COLLATION")),
            Statement::ExplainTable { .. } => Ok(Analysis::bare(StatementKind::Describe, None)),
            Statement::Explain { statement, .. } => {
                let mut inner = self.statement(statement)?;
                inner.kind = StatementKind::Explain(Box::new(inner.kind));
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
            writes: created(cols.cols),
            ..Analysis::bare(StatementKind::CreateTableAs, target)
        })
    }

    /// Resolves `CREATE TABLE`: the new name is the target, and a query feeds its columns.
    fn create_table(&mut self, create: &CreateTable) -> Result<Analysis, LineageError> {
        if create.query.is_some() && !create.columns.is_empty() {
            return Err(unsupported("CREATE TABLE AS with column definitions"));
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
            writes: created(cols.cols),
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
            writes: created(cols.cols),
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

/// Turns a created relation's query columns into writes, one per column name.
fn created(cols: Vec<Col>) -> Vec<TargetColumn> {
    let mut writes: Vec<TargetColumn> = Vec::new();
    for col in cols {
        match writes.iter_mut().find(|w| w.name == col.name) {
            Some(existing) => existing.sources.extend(col.sources),
            None => writes.push(TargetColumn {
                name: col.name,
                sources: col.sources,
            }),
        }
    }
    writes
}

/// Builds a `SHOW` analysis.
fn show(what: &str) -> Analysis {
    Analysis::bare(StatementKind::Show(what.to_string()), None)
}

/// Returns a statement's variant name, which holds no identifiers or values.
fn variant(statement: &Statement) -> String {
    format!("{statement:?}")
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect()
}
