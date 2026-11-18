//! The planner: gates a statement's lineage and plans each output column.

use std::collections::BTreeSet;
use std::str::FromStr;

use dbmcp_config::{ColumnRule, PiiConfig, PiiOperator};
use dbmcp_sql_lineage::{Dialect, Lineage, Origin, OutputColumn, RelationId, Role, SourceRef, StatementKind};

use super::deny;
use super::rejection::Rejection;
use crate::entity::Entity;

/// How the values of each output column are redacted, in result order.
///
/// Positions follow the result set of a query or a write's `RETURNING`
/// only. For `EXPLAIN`, `SHOW`, `DESCRIBE`, and DDL the server returns
/// other columns, so callers detect globally on those rows instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// One entry per output column, by 0-based position.
    pub columns: Vec<PlannedColumn>,
}

/// One output column's redaction and the name it must come back under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedColumn {
    /// How the column's values are redacted.
    pub position: Position,
    /// The folded name the column must come back under, where known.
    ///
    /// `Some` only for a column with one source whose output name is that
    /// source's column name (`*`, a bare column, or a same-name alias): the
    /// server returns it under that name, so a mismatch means the result
    /// shifted since the plan was made. Expression names are engine-specific
    /// and stay `None`.
    pub name: Option<String>,
}

/// How the values of one output column are redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// Replace every non-null value whole with the operator; the entity labels it.
    Action(PiiOperator, Entity),
    /// Run global detection, as without column rules.
    Global,
}

/// Plans a statement's output columns under `config`'s column rules.
///
/// `lineage` must come from a successful analysis; map a failed one with
/// [`reject_lineage`](super::reject_lineage). Each rejection is logged at
/// `info` under the `dbmcp::policy` target.
///
/// # Errors
///
/// Returns the first failing gate's [`Rejection`]: an unknown function, a
/// denied relation or statement, `EXPLAIN` of a ruled relation, `CREATE
/// INDEX` on one, a ruled column used outside the output, conflicting rules
/// on one output column, or a write copying ruled values into a column not
/// ruled the same way or not nameable by a rule.
///
/// # Panics
///
/// Panics when a rule's entity is not an [`Entity`] name; the loader only
/// accepts `ColumnRule::ENTITIES`, which a test ties to [`Entity`].
pub fn plan(lineage: &Lineage, dialect: Dialect, config: &PiiConfig) -> Result<Plan, Rejection> {
    gate(lineage, dialect, config).map_err(Rejection::logged)
}

/// Reports whether `lineage` touches a relation with a ruled column.
///
/// Every relation counts, those named inside view bodies included. Backends
/// use it to decide whether database errors must be sanitized.
#[must_use]
pub fn touches_ruled(lineage: &Lineage, config: &PiiConfig) -> bool {
    ruled_relation(lineage, config).is_some()
}

/// Runs every gate in order.
fn gate(lineage: &Lineage, dialect: Dialect, config: &PiiConfig) -> Result<Plan, Rejection> {
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
        if rule(config, &reference.source).is_none() {
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

/// Returns the folded name a column comes back under, if the server keeps it.
///
/// Known only for one source whose column name the output name repeats.
fn known_name(column: &OutputColumn) -> Option<String> {
    let mut sources = column.sources.iter();
    match (sources.next(), sources.next()) {
        (Some(source), None) if ColumnRule::fold(&column.name) == ColumnRule::fold(&source.column) => {
            Some(ColumnRule::fold(&source.column))
        }
        _ => None,
    }
}

/// Returns the rule whose key is `source`'s folded `catalog.schema.table.column`.
fn rule<'a>(config: &'a PiiConfig, source: &SourceRef) -> Option<&'a ColumnRule> {
    let relation = &source.relation;
    let key = [
        ColumnRule::fold(&relation.catalog),
        ColumnRule::fold(&relation.schema),
        ColumnRule::fold(&relation.name),
        ColumnRule::fold(&source.column),
    ];
    config.rules.iter().find(|rule| rule.key == key)
}

/// Reports whether any rule names a column of `relation`.
fn is_ruled(relation: &RelationId, config: &PiiConfig) -> bool {
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
        .filter_map(|source| rule(config, source))
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
