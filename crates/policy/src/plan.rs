//! The planner: gates a statement's lineage and plans each output column.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use dbmcp_config::{ColumnRule, PiiConfig};
use dbmcp_pii::Entity;
use dbmcp_pii::cells::{Plan, PlannedColumn, Position};
use dbmcp_sql_lineage::{
    Dialect, ExplainForm, Family, Lineage, Member, Origin, OutputColumn, RelationId, Role, SourceRef, StatementKind,
};

use crate::deny;
use crate::rejection::Rejection;

/// Plans a statement's output columns under `config`'s column rules.
///
/// `lineage` must come from a successful analysis; map a failed one with
/// [`reject_lineage`](crate::rejection::reject_lineage). Each rejection is
/// logged at `info` under the `dbmcp::policy` target.
///
/// # Errors
///
/// Returns the first failing gate's [`Rejection`]: an unknown function or
/// cast target type, a denied relation or statement, `EXPLAIN` of a ruled relation other than a
/// plain JSON or query-plan form, `EXPLAIN ANALYZE` on one, `CREATE INDEX` on
/// one, a ruled column used outside the output, conflicting rules on one
/// output column, a shadowed bare create target, or a write copying ruled
/// values into a column not ruled the same way or not nameable by a rule,
/// or an `UPDATE` or `MERGE` through a partitioned table ruled unevenly.
///
/// # Panics
///
/// Panics when a rule's entity is not an [`Entity`] name; the loader only
/// accepts `ColumnRule::ENTITIES`, which a test ties to [`Entity`].
pub(crate) fn plan(lineage: &Lineage, dialect: Dialect, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    gate(lineage, dialect, config, names).map_err(Rejection::logged)
}

/// Bare-name resolutions and the creation schema.
#[derive(Debug, Default)]
pub(crate) struct Names {
    /// The schema unqualified creates land in, if known.
    pub(crate) creation: Option<String>,
    /// Relations that one-part query names resolved to.
    pub(crate) bare: BTreeSet<RelationId>,
    /// Each resolved relation's inheritance tree; a missing one is empty.
    pub(crate) families: Families,
}

/// Reports whether `lineage` touches a relation with a ruled column.
///
/// Every relation counts, those named inside view bodies included, each
/// ruled when its family is. Backends use it to decide whether database
/// errors must be sanitized.
#[must_use]
pub(crate) fn touches_ruled(lineage: &Lineage, config: &PiiConfig, families: &Families) -> bool {
    ruled_relation(lineage, config, families).is_some()
}

/// Refuses a called function or cast target type missing from the dialect's tables.
fn built_ins(lineage: &Lineage) -> Result<(), Rejection> {
    if let Some(function) = lineage.functions.iter().find(|function| !function.known) {
        return Err(Rejection::UnknownFunction {
            name: function.name.clone(),
        });
    }
    if let Some(cast) = lineage.types.iter().find(|cast| !cast.known) {
        return Err(Rejection::UnknownType {
            name: cast.name.clone(),
        });
    }
    Ok(())
}

/// Runs every gate in order.
fn gate(lineage: &Lineage, dialect: Dialect, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    built_ins(lineage)?;
    deny::lineage(dialect, lineage)?;
    let mut statement = &lineage.statement;
    while let StatementKind::Explain(_, inner) = statement {
        statement = inner;
    }
    if let StatementKind::Explain(form, _) = &lineage.statement
        && let Some(relation) = ruled_relation(lineage, config, &names.families)
    {
        let relation = relation_name(relation);
        match form {
            ExplainForm::Json | ExplainForm::QueryPlan => {}
            ExplainForm::Analyze => return Err(Rejection::ExplainAnalyze { relation }),
            ExplainForm::Other => return Err(Rejection::Explain { relation }),
        }
    }
    if *statement == StatementKind::CreateIndex
        && let Some(target) = &lineage.target
        && covers_ruled(config, &names.families, target)
    {
        return Err(Rejection::RuledIndex {
            relation: relation_name(target),
        });
    }
    for reference in &lineage.references {
        let source = &reference.source;
        if covering(config, &names.families, &source.relation, &source.column)
            .next()
            .is_none()
        {
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
                Origin::Query => None,
                Origin::View(view) => Some(relation_name(view)),
            },
        });
    }
    let columns = lineage
        .columns
        .iter()
        .map(|column| {
            let position = combine(&column.sources, config, &names.families).ok_or(Rejection::Conflict {
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
            let source = combine(&write.sources, config, &names.families).ok_or_else(laundering)?;
            if source == Position::Global {
                continue;
            }
            if !write.named {
                return Err(Rejection::Unnamed {
                    relation: relation_name(target),
                    position,
                });
            }
            // Every member a row can land in must read back under the source's rule.
            let empty = Family::default();
            let family = names.families.get(target).unwrap_or(&empty);
            let mut landing = std::iter::once((target, &family.ancestors))
                .chain(family.descendants.iter().map(|member| (&member.id, &member.ancestors)));
            if !landing
                .all(|(member, ancestors)| merge(direct(config, member, ancestors.iter(), &write.name)) == Some(source))
            {
                return Err(laundering());
            }
        }
    }
    row_movement(lineage, config, names)?;
    Ok(Plan { columns })
}

/// Rejects an `UPDATE` or `MERGE` through a partitioned table ruled unevenly.
///
/// A row moved between partitions would be read back under the rules of
/// another leaf, so every leaf must read each ruled column under the same
/// pairs. Moves are decided per row at run time, so this fires even when no
/// row would move. Lineage keeps no per-action detail, so every `MERGE` counts.
fn row_movement(lineage: &Lineage, config: &PiiConfig, names: &Names) -> Result<(), Rejection> {
    if !matches!(lineage.statement, StatementKind::Update | StatementKind::Merge) {
        return Ok(());
    }
    let Some(target) = &lineage.target else {
        return Ok(());
    };
    let Some(family) = names.families.get(target).filter(|family| family.partitioned) else {
        return Ok(());
    };
    let leaves: Vec<_> = family.descendants.iter().filter(|member| !member.partitioned).collect();
    let pairs = |member: &Member, column: &str| {
        direct(config, &member.id, member.ancestors.iter(), column)
            .map(|rule| (rule.operator, rule.entity))
            .collect::<Vec<_>>()
    };
    // A column no leaf reads under a rule yields empty, equal pair lists.
    let uniform = config.rules.iter().all(|rule| {
        let column = &rule.key[3];
        let mut sets = leaves.iter().map(|leaf| pairs(leaf, column));
        sets.next().is_none_or(|first| {
            sets.all(|set| set.iter().all(|pair| first.contains(pair)) && first.iter().all(|pair| set.contains(pair)))
        })
    });
    if uniform {
        return Ok(());
    }
    Err(Rejection::RowMovement {
        relation: relation_name(target),
    })
}

/// Rejects a bare create target resolved outside the creation schema.
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

/// Each resolved relation's inheritance tree, by identity.
pub(crate) type Families = BTreeMap<RelationId, Family>;

/// Returns `relation` and every relation of its family.
fn reach<'f>(families: &'f Families, relation: &'f RelationId) -> impl Iterator<Item = &'f RelationId> {
    std::iter::once(relation).chain(families.get(relation).into_iter().flat_map(Family::relations))
}

/// Returns the rules on `column` of `relation` and of every relation of its family.
///
/// Rows read through a relation then follow every rule a direct read of
/// the member they live in would.
pub(crate) fn covering<'a>(
    config: &'a PiiConfig,
    families: &'a Families,
    relation: &'a RelationId,
    column: &'a str,
) -> impl Iterator<Item = &'a ColumnRule> {
    reach(families, relation).filter_map(move |relation| rule(config, relation, column))
}

/// Returns the rules on `column` of `member` and of its `ancestors`.
///
/// These are the rules a direct read of `member.column` follows.
fn direct<'a>(
    config: &'a PiiConfig,
    member: &'a RelationId,
    ancestors: impl Iterator<Item = &'a RelationId>,
    column: &'a str,
) -> impl Iterator<Item = &'a ColumnRule> {
    std::iter::once(member)
        .chain(ancestors)
        .filter_map(move |relation| rule(config, relation, column))
}

/// Reports whether any rule names a column of `relation`.
pub(crate) fn is_ruled(relation: &RelationId, config: &PiiConfig) -> bool {
    config.rules.iter().any(|rule| rule_on(rule, relation))
}

/// Reports whether `rule` names a column of `relation`.
fn rule_on(rule: &ColumnRule, relation: &RelationId) -> bool {
    rule.key[..3]
        == [
            ColumnRule::fold(&relation.catalog),
            ColumnRule::fold(&relation.schema),
            ColumnRule::fold(&relation.name),
        ]
}

/// Reports whether any rule names a column of `relation` or of its family.
pub(crate) fn covers_ruled(config: &PiiConfig, families: &Families, relation: &RelationId) -> bool {
    reach(families, relation).any(|relation| is_ruled(relation, config))
}

/// Returns the first touched relation whose family has a ruled column.
fn ruled_relation<'a>(lineage: &'a Lineage, config: &PiiConfig, families: &Families) -> Option<&'a RelationId> {
    lineage
        .relations
        .iter()
        .map(|relation| &relation.id)
        .find(|id| covers_ruled(config, families, id))
}

/// Combines the rules of `sources`; `None` when two ruled sources differ.
///
/// Ruled sources decide the position; unruled ones add nothing. No ruled
/// source means global detection.
fn combine(sources: &BTreeSet<SourceRef>, config: &PiiConfig, families: &Families) -> Option<Position> {
    merge(
        sources
            .iter()
            .flat_map(|source| covering(config, families, &source.relation, &source.column)),
    )
}

/// Combines `rules` into one position; `None` when two pairs differ.
///
/// No rule means global detection.
fn merge<'a>(rules: impl Iterator<Item = &'a ColumnRule>) -> Option<Position> {
    let mut pairs = rules.map(|rule| (rule.operator, rule.entity));
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
pub(crate) fn relation_name(id: &RelationId) -> String {
    format!("{}.{}", id.schema, id.name)
}

/// Renders a column as `schema.table.column`.
fn column_name(source: &SourceRef) -> String {
    format!("{}.{}.{}", source.relation.schema, source.relation.name, source.column)
}
