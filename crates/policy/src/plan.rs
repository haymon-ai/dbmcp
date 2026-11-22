//! The planner: gates a statement's lineage and plans each output column.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use dbmcp_config::{ColumnRule, PiiConfig, PiiOperator};
use dbmcp_pii::Entity;
use dbmcp_pii::cells::{Plan, PlannedColumn, Position};
use dbmcp_sql_lineage::{
    Dialect, ExplainForm, Flow, Lineage, Origin, OutputColumn, RelationId, Role, SourceRef, StatementKind,
};

use crate::catalog::{Cascade, CascadeAction, CatalogColumn, Family, Member};
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
/// Returns the first failing gate's [`Rejection`]; the crate docs list the gates in order.
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
    /// Each resolved relation's write facts; a missing one is all `true`.
    pub(crate) facts: BTreeMap<RelationId, Facts>,
}

/// What a write can make the server put into a relation's columns.
#[derive(Debug)]
pub(crate) struct Facts {
    /// The relation's columns with their default, generated, and on-update facts.
    pub(crate) columns: Vec<CatalogColumn>,
    /// Whether the relation has a user trigger or a non-`SELECT` rule.
    pub(crate) triggers: bool,
    /// Referential actions that change rows of other tables, transitively.
    pub(crate) cascades: Vec<Cascade>,
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

/// Runs every gate in order.
fn gate(lineage: &Lineage, dialect: Dialect, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    if let Some(function) = lineage.functions.iter().find(|function| !function.known) {
        return Err(Rejection::UnknownFunction {
            name: function.name.clone(),
        });
    }
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
            let position =
                combine(&column.sources, column.flow, config, &names.families).ok_or(Rejection::Conflict {
                    position: column.position,
                })?;
            Ok(PlannedColumn {
                position,
                name: known_name(column),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    create_target(lineage, names)?;
    let family = lineage.target.as_ref().and_then(|target| names.families.get(target));
    let effects = effects(statement, family.is_some_and(|family| family.partitioned));
    if *statement != StatementKind::CreateView
        && let Some(target) = &lineage.target
    {
        writes(lineage, effects, target, config, names)?;
    }
    row_movement(lineage, effects, config, names)?;
    Ok(Plan { columns })
}

/// Gates written columns, server-filled `hash` columns, then cascades and triggers.
///
/// A ruled value must land in a column every landing member reads back
/// under the same pair; a `hash` token must never sign a value the
/// statement could steer.
fn writes(
    lineage: &Lineage,
    effects: Effects,
    target: &RelationId,
    config: &PiiConfig,
    names: &Names,
) -> Result<(), Rejection> {
    let empty = Family::default();
    let family = names.families.get(target).unwrap_or(&empty);
    let landing = || {
        std::iter::once((target, &family.ancestors))
            .chain(family.descendants.iter().map(|member| (&member.id, &member.ancestors)))
    };
    let hashed = |column: &str| {
        landing().any(|(member, ancestors)| {
            direct(config, member, ancestors.iter(), column).any(|rule| rule.operator == PiiOperator::Hash)
        })
    };
    // A `hash`-ruled column the facts lack (no facts, or a descendant's own
    // column) has every fact.
    let mut columns = names
        .facts
        .get(target)
        .map(|facts| facts.columns.clone())
        .unwrap_or_default();
    for rule in &config.rules {
        let name = &rule.key[3];
        if rule.operator == PiiOperator::Hash
            && hashed(name)
            && !columns.iter().any(|column| ColumnRule::fold(&column.name) == *name)
        {
            columns.push(CatalogColumn {
                name: name.clone(),
                star_visible: true,
                default: true,
                generated: true,
                on_update: true,
            });
        }
    }
    // The exact name first; else every column it folds onto, `PostgreSQL` names differing in case included.
    let defaults = |name: &str| {
        if let Some(column) = columns.iter().find(|column| column.name == name) {
            return column.default;
        }
        let mut folded = columns
            .iter()
            .filter(|column| ColumnRule::fold(&column.name) == ColumnRule::fold(name))
            .peekable();
        folded.peek().is_none() || folded.any(|column| column.default)
    };
    let computed = |column: &str| Rejection::Computed {
        column: column_name(&SourceRef::new(target.clone(), column)),
    };
    for (position, write) in lineage.writes.iter().enumerate() {
        let column = SourceRef::new(target.clone(), &write.name);
        let laundering = || {
            if write.named {
                Rejection::Laundering {
                    column: column_name(&column),
                }
            } else {
                Rejection::UnnamedLaundering {
                    relation: relation_name(target),
                    position,
                }
            }
        };
        // Literals and unruled columns carry no ruled values.
        let source = combine(&write.sources, write.flow, config, &names.families).ok_or_else(laundering)?;
        // A computed ruled value never lands under a rule, so an alias would not help.
        if matches!(source, Position::Placeholder(_)) {
            return Err(laundering());
        }
        if source != Position::Global {
            if !write.named {
                return Err(Rejection::Unnamed {
                    relation: relation_name(target),
                    position,
                });
            }
            // Every member a row can land in must read back under the source's rule.
            if !landing().all(|(member, ancestors)| {
                merge(direct(config, member, ancestors.iter(), &write.name).map(pair)) == Some(source)
            }) {
                return Err(laundering());
            }
        }
        // The landing check proved a `hash` source equal to every member's pair.
        if !hashed(&write.name) || matches!(source, Position::Action(PiiOperator::Hash, _)) {
            continue;
        }
        match write.flow {
            Flow::Null => {}
            Flow::Default if !defaults(&write.name) => {}
            Flow::Default => return Err(computed(&write.name)),
            Flow::Bare | Flow::Derived => {
                return Err(Rejection::Chosen {
                    column: column_name(&column),
                });
            }
        }
    }
    for column in columns.iter().filter(|column| hashed(&column.name)) {
        if (effects.omits && column.default) || (effects.fills && (column.generated || column.on_update)) {
            return Err(computed(&column.name));
        }
    }
    if !config.rules.iter().any(|rule| rule.operator == PiiOperator::Hash) {
        return Ok(());
    }
    let members: BTreeSet<&RelationId> = landing().map(|(member, _)| member).collect();
    cascades_and_triggers(lineage, effects, target, &members, config, names)
}

/// What a statement does to the rows of its target, as the write gates read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Effects {
    /// It writes rows, so the server recomputes generated and on-update columns.
    pub(crate) fills: bool,
    /// It may leave a column to the server's default.
    pub(crate) omits: bool,
    /// It fires the update edges of the columns it writes.
    pub(crate) updates: bool,
    /// It removes rows, firing every delete edge.
    pub(crate) deletes: bool,
    /// It fires the target's triggers and rules.
    pub(crate) triggers: bool,
    /// It may move rows between the partitions of a partitioned target.
    pub(crate) moves: bool,
}

/// Returns what `statement`, unwrapped from any `EXPLAIN`, does to its target's rows.
///
/// Lineage keeps no per-clause detail: an upsert's conflict clause and a
/// `MERGE` clause write columns its insert may omit, so every `INSERT` and
/// `MERGE` may leave any column to its default. An upsert updates and a
/// `REPLACE` deletes, so every `INSERT` fires both edge kinds; so does every
/// `MERGE`, and an `UPDATE` through a `partitioned` table, which may move
/// rows by deleting them (`PostgreSQL` before 15). A kind not listed here
/// has every effect (fail closed).
pub(crate) fn effects(statement: &StatementKind, partitioned: bool) -> Effects {
    let none = Effects {
        fills: false,
        omits: false,
        updates: false,
        deletes: false,
        triggers: false,
        moves: false,
    };
    match statement {
        StatementKind::Insert => Effects {
            fills: true,
            omits: true,
            updates: true,
            deletes: true,
            triggers: true,
            ..none
        },
        StatementKind::Update => Effects {
            fills: true,
            updates: true,
            deletes: partitioned,
            triggers: true,
            moves: true,
            ..none
        },
        StatementKind::Delete => Effects {
            deletes: true,
            triggers: true,
            ..none
        },
        StatementKind::Truncate => Effects { triggers: true, ..none },
        // A created relation is new: no defaults, generated columns, or triggers.
        StatementKind::CreateTableAs
        | StatementKind::CreateMaterializedView
        | StatementKind::Query
        | StatementKind::Show(_)
        | StatementKind::Describe
        | StatementKind::CreateView
        | StatementKind::CreateTable
        | StatementKind::Drop
        | StatementKind::CreateIndex => none,
        // `MERGE` may do all of it; a kind added to lineage later is refused until listed.
        _ => Effects {
            fills: true,
            omits: true,
            updates: true,
            deletes: true,
            triggers: true,
            moves: true,
        },
    }
}

/// Rejects writes whose cascades or triggers may fill a `hash`-ruled column.
///
/// A fired `SET DEFAULT`, or a copy from a parent not hashed under the
/// child's pair, fills the child with a value the statement may have
/// steered. A cascade that changes a child column also recomputes the
/// child's generated and on-update columns, whose facts are unknown, so it
/// refuses while any other column of the child is `hash`-ruled. Triggers of
/// the target, or of a fired cascade's child, may write anything; a
/// `TRUNCATE` fires its target's.
fn cascades_and_triggers(
    lineage: &Lineage,
    effects: Effects,
    target: &RelationId,
    members: &BTreeSet<&RelationId>,
    config: &PiiConfig,
    names: &Names,
) -> Result<(), Rejection> {
    // A target the analysis did not resolve has triggers.
    let (triggers, cascades) = names
        .facts
        .get(target)
        .map_or((true, &[][..]), |facts| (facts.triggers, facts.cascades.as_slice()));
    let fired = fired(lineage, effects, members, cascades);
    for edge in &fired {
        if edge.action != CascadeAction::Delete
            && let Some(other) = config.rules.iter().find(|rule| {
                rule.operator == PiiOperator::Hash
                    && rule_on(rule, &edge.child.relation)
                    && rule.key[3] != ColumnRule::fold(&edge.child.column)
            })
        {
            return Err(Rejection::Computed {
                column: column_name(&SourceRef::new(edge.child.relation.clone(), &other.key[3])),
            });
        }
        let Some(child) =
            rule(config, &edge.child.relation, &edge.child.column).filter(|rule| rule.operator == PiiOperator::Hash)
        else {
            continue;
        };
        let refused = match edge.action {
            // The write rule limits the parent's new value to copies hashed like it.
            CascadeAction::Copy => {
                let mut parent =
                    covering(config, &names.families, &edge.parent.relation, &edge.parent.column).peekable();
                parent.peek().is_none() || parent.any(|rule| pair(rule) != pair(child))
            }
            CascadeAction::Default => true,
            CascadeAction::Null | CascadeAction::Delete => false,
        };
        if refused {
            return Err(Rejection::Computed {
                column: column_name(&edge.child),
            });
        }
    }
    if effects.triggers && triggers {
        return Err(Rejection::Trigger {
            relation: relation_name(target),
        });
    }
    if let Some(edge) = fired.iter().find(|edge| edge.triggers) {
        return Err(Rejection::Trigger {
            relation: relation_name(&edge.child.relation),
        });
    }
    Ok(())
}

/// Returns the cascade edges a write through `members` fires, transitively.
///
/// An update edge fires when the statement updates its parent column, a
/// delete edge when it deletes; below a fired edge, a changed child column
/// fires its update edges and a removed child row every delete edge.
/// Case-folded names may over-match (fail closed).
fn fired<'a>(
    lineage: &Lineage,
    effects: Effects,
    members: &BTreeSet<&RelationId>,
    cascades: &'a [Cascade],
) -> Vec<&'a Cascade> {
    let Effects { updates, deletes, .. } = effects;
    let same = |a: &str, b: &str| ColumnRule::fold(a) == ColumnRule::fold(b);
    let relation = |a: &RelationId, b: &RelationId| {
        same(&a.catalog, &b.catalog) && same(&a.schema, &b.schema) && same(&a.name, &b.name)
    };
    let mut seen = vec![false; cascades.len()];
    let mut fired: Vec<&Cascade> = Vec::new();
    for (index, edge) in cascades.iter().enumerate() {
        let starts = members.iter().any(|member| relation(member, &edge.parent.relation))
            && if edge.on_delete {
                deletes
            } else {
                updates
                    && lineage
                        .writes
                        .iter()
                        .any(|write| same(&write.name, &edge.parent.column))
            };
        if starts {
            seen[index] = true;
            fired.push(edge);
        }
    }
    let mut next = 0;
    while let Some(&edge) = fired.get(next) {
        next += 1;
        for (index, below) in cascades.iter().enumerate() {
            let follows = relation(&below.parent.relation, &edge.child.relation)
                && match edge.action {
                    CascadeAction::Delete => below.on_delete,
                    CascadeAction::Copy | CascadeAction::Null | CascadeAction::Default => {
                        !below.on_delete && same(&below.parent.column, &edge.child.column)
                    }
                };
            if follows && !seen[index] {
                seen[index] = true;
                fired.push(below);
            }
        }
    }
    fired
}

/// Rejects an `UPDATE` or `MERGE` through a partitioned table ruled unevenly.
///
/// A row moved between partitions would be read back under the rules of
/// another leaf, so every leaf must read each ruled column under the same
/// pairs. Moves are decided per row at run time, so this fires even when no
/// row would move.
fn row_movement(lineage: &Lineage, effects: Effects, config: &PiiConfig, names: &Names) -> Result<(), Rejection> {
    if !effects.moves {
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

/// Combines the rules of `sources` reaching a value by `flow`; `None` when two pairs differ.
///
/// Ruled sources decide the position; unruled ones add nothing. No ruled
/// source means global detection. A value not [`Flow::Bare`] over a ruled
/// source is the entity's placeholder, `NULL` included, whatever the
/// operators, and its ruled sources must share the entity: a condition in
/// the select list would otherwise answer a refused `WHERE` through
/// `null`, distinct tokens, or `mask` length. A `hash` pair survives only
/// on a bare value whose every source is covered by that same pair;
/// otherwise each `hash` pair turns into `replace` before merging, so a
/// set operation never gets the token of a value its query chose.
fn combine(sources: &BTreeSet<SourceRef>, flow: Flow, config: &PiiConfig, families: &Families) -> Option<Position> {
    let pairs: Vec<Vec<(PiiOperator, &str)>> = sources
        .iter()
        .map(|source| {
            covering(config, families, &source.relation, &source.column)
                .map(pair)
                .collect()
        })
        .collect();
    let first = pairs.iter().flatten().next().copied();
    if flow != Flow::Bare
        && let Some((_, entity)) = first
    {
        if !pairs.iter().flatten().all(|&(_, other)| other == entity) {
            return None;
        }
        let entity = Entity::from_str(entity).expect("ColumnRule::ENTITIES mirrors Entity");
        return Some(Position::Placeholder(entity));
    }
    let hashed = flow == Flow::Bare
        && first.is_some_and(|first| first.0 == PiiOperator::Hash)
        && pairs
            .iter()
            .all(|covered| !covered.is_empty() && covered.iter().all(|&p| Some(p) == first));
    merge(pairs.into_iter().flatten().map(|(operator, entity)| match operator {
        PiiOperator::Hash if !hashed => (PiiOperator::Replace, entity),
        operator => (operator, entity),
    }))
}

/// Returns a rule's `(operator, entity)` pair.
fn pair(rule: &ColumnRule) -> (PiiOperator, &'static str) {
    (rule.operator, rule.entity)
}

/// Combines `(operator, entity)` pairs into one position; `None` when two pairs differ.
///
/// No pair means global detection.
fn merge<'a>(mut pairs: impl Iterator<Item = (PiiOperator, &'a str)>) -> Option<Position> {
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
