//! The planner: gates a statement's lineage and plans each output column.

use std::collections::BTreeSet;
use std::str::FromStr;

use dbmcp_config::{ColumnRule, PiiConfig};
use dbmcp_pii::Entity;
use dbmcp_pii::cells::{Plan, PlannedColumn, Position};
use dbmcp_sql_lineage::{Dialect, Lineage, Origin, OutputColumn, RelationId, Role, SourceRef, StatementKind};

use crate::deny;
use crate::rejection::Rejection;

/// Plans a statement's output columns under `config`'s column rules.
///
/// `lineage` must come from a successful analysis; map a failed one with
/// [`reject_lineage`](crate::rejection::reject_lineage). Each rejection is logged at
/// `info` under the `dbmcp::policy` target.
///
/// # Errors
///
/// Returns the first failing gate's [`Rejection`]: an unknown function, a
/// denied relation or statement, `EXPLAIN` of a ruled relation, `CREATE
/// INDEX` on one, a ruled column used outside the output, conflicting rules
/// on one output column, a shadowed bare create target, or a write copying
/// ruled values into a column not ruled the same way or not nameable by a rule.
///
/// # Panics
///
/// Panics when a rule's entity is not an [`Entity`] name; the loader only
/// accepts `ColumnRule::ENTITIES`, which a test ties to [`Entity`].
pub(crate) fn plan(lineage: &Lineage, dialect: Dialect, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    gate(lineage, dialect, config, names).map_err(Rejection::logged)
}

/// Where a statement's bare names resolved, and where the server creates.
#[derive(Debug, Default)]
pub(crate) struct Names {
    /// The schema an unqualified create lands in; `None` when unknown.
    pub(crate) creation: Option<String>,
    /// The relations a one-part name written in the statement resolved to.
    pub(crate) bare: BTreeSet<RelationId>,
}

/// Reports whether `lineage` touches a relation with a ruled column.
///
/// Every relation counts, those named inside view bodies included. Backends
/// use it to decide whether database errors must be sanitized.
#[must_use]
pub(crate) fn touches_ruled(lineage: &Lineage, config: &PiiConfig) -> bool {
    ruled_relation(lineage, config).is_some()
}

/// Runs every gate in order.
fn gate(lineage: &Lineage, dialect: Dialect, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    if let Some(function) = lineage.functions.iter().find(|function| !function.known) {
        return Err(Rejection::UnknownFunction {
            name: function.name.clone(),
        });
    }
    if let Some(relation) = lineage
        .relations
        .iter()
        .find(|relation| deny::relation(dialect, relation))
    {
        return Err(Rejection::DeniedRelation {
            relation: relation_name(&relation.id),
        });
    }
    let mut statement = &lineage.statement;
    while let StatementKind::Explain(inner) = statement {
        statement = inner;
    }
    if let StatementKind::Show(what) = statement
        && deny::show(dialect, what)
    {
        return Err(Rejection::DeniedStatement {
            statement: what.clone(),
        });
    }
    if matches!(lineage.statement, StatementKind::Explain(_))
        && let Some(relation) = ruled_relation(lineage, config)
    {
        return Err(Rejection::Explain {
            relation: relation_name(relation),
        });
    }
    if *statement == StatementKind::CreateIndex
        && let Some(target) = &lineage.target
        && is_ruled(target, config)
    {
        return Err(Rejection::RuledIndex {
            relation: relation_name(target),
        });
    }
    for reference in &lineage.references {
        if rule(config, &reference.source.relation, &reference.source.column).is_none() {
            continue;
        }
        return Err(Rejection::Reference {
            column: column_name(&reference.source),
            role: match reference.role {
                Role::Join => "JOIN",
                Role::Filter => "WHERE",
                Role::GroupBy => "GROUP BY",
                Role::Sort => "ORDER BY",
                Role::Window => "a window",
            },
            view: match &reference.origin {
                Origin::Query => String::new(),
                Origin::View(view) => format!(" in view \"{}\"", relation_name(view)),
            },
        });
    }
    let columns = lineage
        .columns
        .iter()
        .map(|column| {
            let position = combine(&column.sources, config).ok_or(Rejection::Conflict {
                position: column.position,
            })?;
            Ok(PlannedColumn {
                position,
                name: known_name(column),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    create_target(lineage, names)?;
    if *statement != StatementKind::CreateView
        && let Some(target) = &lineage.target
    {
        for (position, write) in lineage.writes.iter().enumerate() {
            let column = SourceRef::new(target.clone(), &write.name);
            let laundering = || Rejection::Laundering {
                column: column_name(&column),
            };
            // Literals and unruled columns carry no ruled values.
            let source = combine(&write.sources, config).ok_or_else(laundering)?;
            if source == Position::Global {
                continue;
            }
            if !write.named {
                return Err(Rejection::Unnamed {
                    relation: relation_name(target),
                    position,
                });
            }
            let own = combine(&BTreeSet::from([column.clone()]), config).ok_or_else(laundering)?;
            if own != source {
                return Err(laundering());
            }
        }
    }
    Ok(Plan { columns })
}

/// Rejects a create whose bare target name resolved outside the creation schema.
///
/// `EXPLAIN` creates nothing, so only the top-level statement counts.
fn create_target(lineage: &Lineage, names: &Names) -> Result<(), Rejection> {
    if matches!(
        lineage.statement,
        StatementKind::CreateTableAs | StatementKind::CreateView | StatementKind::CreateMaterializedView
    ) && let Some(target) = &lineage.target
        && names.bare.contains(target)
        && names.creation.as_deref() != Some(target.schema.as_str())
    {
        return Err(Rejection::ShadowedTarget {
            relation: relation_name(target),
        });
    }
    Ok(())
}

/// Computes [`PlannedColumn::name`].
fn known_name(column: &OutputColumn) -> Option<String> {
    let mut sources = column.sources.iter();
    let (Some(source), None) = (sources.next(), sources.next()) else {
        return None;
    };
    let folded = ColumnRule::fold(&source.column);
    (ColumnRule::fold(&column.name) == folded).then_some(folded)
}

/// Returns the rule whose key is the folded `catalog.schema.table.column`.
#[must_use]
pub(crate) fn rule<'a>(config: &'a PiiConfig, relation: &RelationId, column: &str) -> Option<&'a ColumnRule> {
    let key = [
        ColumnRule::fold(&relation.catalog),
        ColumnRule::fold(&relation.schema),
        ColumnRule::fold(&relation.name),
        ColumnRule::fold(column),
    ];
    config.rules.iter().find(|rule| rule.key == key)
}

/// Reports whether any rule names a column of `relation`.
pub(crate) fn is_ruled(relation: &RelationId, config: &PiiConfig) -> bool {
    let key = [
        ColumnRule::fold(&relation.catalog),
        ColumnRule::fold(&relation.schema),
        ColumnRule::fold(&relation.name),
    ];
    config.rules.iter().any(|rule| rule.key[..3] == key)
}

/// Returns the first touched relation with a ruled column.
fn ruled_relation<'a>(lineage: &'a Lineage, config: &PiiConfig) -> Option<&'a RelationId> {
    lineage
        .relations
        .iter()
        .map(|relation| &relation.id)
        .find(|id| is_ruled(id, config))
}

/// Combines the rules of `sources`; `None` when two ruled sources differ.
///
/// Ruled sources decide the position; unruled ones add nothing. No ruled
/// source means global detection.
fn combine(sources: &BTreeSet<SourceRef>, config: &PiiConfig) -> Option<Position> {
    let mut pairs = sources
        .iter()
        .filter_map(|source| rule(config, &source.relation, &source.column))
        .map(|rule| (rule.operator, rule.entity));
    let Some(first) = pairs.next() else {
        return Some(Position::Global);
    };
    if !pairs.all(|pair| pair == first) {
        return None;
    }
    let entity = Entity::from_str(first.1).expect("ColumnRule::ENTITIES mirrors Entity");
    Some(Position::Action(first.0, entity))
}

/// Renders a relation as `schema.name`.
fn relation_name(id: &RelationId) -> String {
    format!("{}.{}", id.schema, id.name)
}

/// Renders a column as `schema.table.column`.
fn column_name(source: &SourceRef) -> String {
    format!("{}.{}.{}", source.relation.schema, source.relation.name, source.column)
}
