//! Column lineage for write statements (INSERT/UPDATE/DELETE/CTAS/MERGE).
//!
//! Each extractor records the write `target` and maps its target columns back
//! to source columns, reusing the scope and expression engine from [`resolve`].
//! Resolution stays fail-closed: any unresolvable column aborts the statement.

use std::collections::BTreeSet;

use sqlparser::ast::{
    Assignment, AssignmentTarget, CreateTable, Delete, FromTable, Insert, Merge, MergeAction, MergeInsertKind,
    ObjectName, TableFactor, TableObject, Update, UpdateTableFromKind,
};

use crate::error::LineageError;
use crate::model::{Lineage, OutputColumn, SourceRef, TableRef};
use crate::resolve::{
    self, ActiveCtes, CteMap, add_table_factor, add_table_with_joins, collect_sources, last_ident,
    table_ref_from_object_name,
};
use crate::scope::Scope;

/// Extracts lineage from an `INSERT`, including `INSERT … SELECT/VALUES/SET`.
///
/// # Errors
/// Returns [`LineageError`] on a non-table target, count mismatch, or unresolved column.
pub(crate) fn extract_insert(insert: &Insert) -> Result<Lineage, LineageError> {
    let target = target_table(&insert.table)?;

    let columns = if let Some(query) = &insert.source {
        let source = resolve::resolve(query)?;
        if insert.columns.is_empty() {
            source
        } else {
            map_columns(&target, &target_column_names(&insert.columns), source)?
        }
    } else if !insert.assignments.is_empty() {
        resolve_assignments(&insert.assignments, &Scope::default())?
    } else {
        Vec::new()
    };

    Ok(Lineage::from_write(target, columns, BTreeSet::new()))
}

/// Extracts lineage from `CREATE TABLE … AS SELECT`.
///
/// Returns an empty lineage for a plain `CREATE TABLE` without a query.
///
/// # Errors
/// Returns [`LineageError`] on count mismatch or an unresolved source column.
pub(crate) fn extract_create_table(ct: &CreateTable) -> Result<Lineage, LineageError> {
    let Some(query) = &ct.query else {
        return Ok(Lineage::empty());
    };
    let target = table_ref_from_object_name(&ct.name);
    let source = resolve::resolve(query)?;
    let columns = if ct.columns.is_empty() {
        source
    } else {
        let names: Vec<String> = ct.columns.iter().map(|c| c.name.value.clone()).collect();
        map_columns(&target, &names, source)?
    };
    Ok(Lineage::from_write(target, columns, BTreeSet::new()))
}

/// Extracts lineage from `UPDATE t SET col = expr [FROM src]`.
///
/// # Errors
/// Returns [`LineageError`] on a non-table target or unresolved assignment.
pub(crate) fn extract_update(update: &Update) -> Result<Lineage, LineageError> {
    let target = table_ref_from_table_factor(&update.table.relation)?;

    let ctes = CteMap::new();
    let active = ActiveCtes::new();
    let mut scope = Scope::default();
    add_table_with_joins(&update.table, &mut scope, &ctes, &active, 0)?;
    if let Some(from) = &update.from {
        for twj in update_from_tables(from) {
            add_table_with_joins(twj, &mut scope, &ctes, &active, 0)?;
        }
    }

    let columns = resolve_assignments(&update.assignments, &scope)?;
    Ok(Lineage::from_write(target, columns, BTreeSet::new()))
}

/// Extracts lineage from `DELETE FROM t [USING src]` (table-level only).
///
/// # Errors
/// Returns [`LineageError`] on a non-table target or unresolved `USING` relation.
pub(crate) fn extract_delete(delete: &Delete) -> Result<Lineage, LineageError> {
    let target = if let Some(name) = delete.tables.first() {
        table_ref_from_object_name(name)
    } else {
        let twjs = from_table_tables(&delete.from);
        let first = twjs.first().ok_or_else(|| LineageError::UnknownTable {
            table: "<delete target>".to_string(),
        })?;
        table_ref_from_table_factor(&first.relation)?
    };

    let ctes = CteMap::new();
    let active = ActiveCtes::new();
    let mut scope = Scope::default();
    if let Some(using) = &delete.using {
        for twj in using {
            add_table_with_joins(twj, &mut scope, &ctes, &active, 0)?;
        }
    }
    Ok(Lineage::from_write(target, Vec::new(), scope.physical_tables()))
}

/// Extracts lineage from `MERGE INTO t USING src … WHEN … THEN INSERT/UPDATE`.
///
/// # Errors
/// Returns [`LineageError`] on a non-table target/source or unresolved column.
pub(crate) fn extract_merge(merge: &Merge) -> Result<Lineage, LineageError> {
    let target = table_ref_from_table_factor(&merge.table)?;

    let ctes = CteMap::new();
    let active = ActiveCtes::new();
    let mut scope = Scope::default();
    add_table_factor(&merge.table, &mut scope, &ctes, &active, 0)?;
    add_table_factor(&merge.source, &mut scope, &ctes, &active, 0)?;

    let mut columns: Vec<OutputColumn> = Vec::new();
    for clause in &merge.clauses {
        match &clause.action {
            MergeAction::Update(update) => {
                for assignment in &update.assignments {
                    let (name, sources) = resolve_assignment(assignment, &scope)?;
                    union_column(&mut columns, Some(name), sources);
                }
            }
            MergeAction::Insert(insert) => {
                if let MergeInsertKind::Values(values) = &insert.kind {
                    let names = target_column_names(&insert.columns);
                    if let Some(row) = values.rows.first() {
                        for (name, expr) in names.iter().zip(&row.content) {
                            let sources = collect_sources(expr, &scope, &ctes, &active, 0)?;
                            union_column(&mut columns, Some(name.clone()), sources);
                        }
                    }
                }
            }
            MergeAction::Delete { .. } => {}
        }
    }

    Ok(Lineage::from_write(target, columns, BTreeSet::new()))
}

/// Resolves a list of assignments into one output column each.
fn resolve_assignments(assignments: &[Assignment], scope: &Scope) -> Result<Vec<OutputColumn>, LineageError> {
    let mut out: Vec<OutputColumn> = Vec::new();
    for assignment in assignments {
        let (name, sources) = resolve_assignment(assignment, scope)?;
        let position = out.len();
        out.push(OutputColumn {
            name: Some(name),
            position,
            sources,
        });
    }
    Ok(out)
}

/// Resolves one `col = expr` assignment to its target name and source columns.
fn resolve_assignment(assignment: &Assignment, scope: &Scope) -> Result<(String, BTreeSet<SourceRef>), LineageError> {
    let name = assignment_target_name(&assignment.target)?;
    let sources = collect_sources(&assignment.value, scope, &CteMap::new(), &ActiveCtes::new(), 0)?;
    Ok((name, sources))
}

/// Returns the assigned column name; tuple targets are unsupported (fail closed).
fn assignment_target_name(target: &AssignmentTarget) -> Result<String, LineageError> {
    match target {
        AssignmentTarget::ColumnName(name) => Ok(last_ident(name).to_string()),
        AssignmentTarget::Tuple(_) => Err(LineageError::UnresolvedColumn {
            column: "(tuple)".to_string(),
            reason: "tuple assignment unsupported".to_string(),
        }),
    }
}

/// Maps an explicit target column list onto a source projection, positionally.
///
/// Fails closed with [`LineageError::ColumnCountMismatch`] on length mismatch.
fn map_columns(
    target: &TableRef,
    names: &[String],
    source: Vec<OutputColumn>,
) -> Result<Vec<OutputColumn>, LineageError> {
    if names.len() != source.len() {
        return Err(LineageError::ColumnCountMismatch {
            target: target.name.clone(),
            expected: names.len(),
            found: source.len(),
        });
    }
    Ok(source
        .into_iter()
        .zip(names)
        .enumerate()
        .map(|(position, (col, name))| OutputColumn {
            name: Some(name.clone()),
            position,
            sources: col.sources,
        })
        .collect())
}

/// Adds sources under a target column, merging into an existing same-named one.
fn union_column(columns: &mut Vec<OutputColumn>, name: Option<String>, sources: BTreeSet<SourceRef>) {
    if let Some(n) = &name
        && let Some(existing) = columns
            .iter_mut()
            .find(|c| c.name.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(n)))
    {
        existing.sources.extend(sources);
        return;
    }
    let position = columns.len();
    columns.push(OutputColumn {
        name,
        position,
        sources,
    });
}

/// Extracts the bare column names from an explicit target column list.
fn target_column_names(columns: &[ObjectName]) -> Vec<String> {
    columns.iter().map(|c| last_ident(c).to_string()).collect()
}

/// Resolves an `INSERT`/`MERGE` target object into a [`TableRef`].
fn target_table(obj: &TableObject) -> Result<TableRef, LineageError> {
    match obj {
        TableObject::TableName(name) => Ok(table_ref_from_object_name(name)),
        _ => Err(LineageError::UnknownTable {
            table: "<non-table insert target>".to_string(),
        }),
    }
}

/// Resolves a `TableFactor` write target into a [`TableRef`] (plain tables only).
fn table_ref_from_table_factor(factor: &TableFactor) -> Result<TableRef, LineageError> {
    match factor {
        TableFactor::Table { name, .. } => Ok(table_ref_from_object_name(name)),
        _ => Err(LineageError::UnknownTable {
            table: "<non-table target>".to_string(),
        }),
    }
}

/// Returns the relations in an `UPDATE … FROM` clause.
fn update_from_tables(kind: &UpdateTableFromKind) -> &[sqlparser::ast::TableWithJoins] {
    match kind {
        UpdateTableFromKind::BeforeSet(tables) | UpdateTableFromKind::AfterSet(tables) => tables,
    }
}

/// Returns the relations in a `DELETE`'s `FROM` clause.
fn from_table_tables(from: &FromTable) -> &[sqlparser::ast::TableWithJoins] {
    match from {
        FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables,
    }
}
