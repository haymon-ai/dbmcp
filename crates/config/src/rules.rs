//! Casts and checks one config file's `pii.rules` map.
//!
//! `pii.rules` is a map of named rules; each lists exact
//! `catalog.schema.table.column` keys with an entity and an optional
//! operator. A file's value is checked whole: names, fields, key shape, the
//! entity, the operator, and duplicates across all of its rules. Reasons are
//! value-free: they name the rule and the item index, never a column key.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_saphyr::{Location, Spanned};

use crate::env;
use crate::node::Node;
use crate::pii::{ColumnRule, PiiOperator};

/// One named rule as a config file wrote it, with folded keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The rule's name.
    pub name: String,
    /// Each listed column's folded `catalog.schema.table.column` segments.
    pub columns: Vec<[String; 4]>,
    /// The entity, one of [`ColumnRule::ENTITIES`].
    pub entity: &'static str,
    /// The rule's operator; `None` inherits `pii.operator`.
    pub operator: Option<PiiOperator>,
}

/// The fields a rule may set.
const FIELDS: &str = "columns, entity, operator";

/// Casts a `pii.rules` mapping into its rules, in file order.
///
/// # Errors
///
/// Returns the first value-free reason, prefixed with the rule's name: a
/// bad name, a rule that is not a mapping, an unknown or missing field, an
/// empty or malformed column list, an unknown entity or operator, or a
/// column listed twice in one rule or in two rules.
pub(crate) fn cast(entries: &IndexMap<String, (Location, Spanned<Node>)>) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::with_capacity(entries.len());
    let mut seen: HashMap<[String; 4], (usize, usize)> = HashMap::new();
    for (index, (name, (_, node))) in entries.iter().enumerate() {
        let rule = rule(name, &node.value).map_err(|reason| format!("{name}: {reason}"))?;
        for (item, key) in rule.columns.iter().enumerate() {
            match seen.get(key) {
                Some(&(other, first)) if other == index => {
                    return Err(format!("{name}: columns: item {item} repeats item {first}"));
                }
                Some(&(other, _)) => {
                    let other = entries.get_index(other).map_or("", |(other, _)| other.as_str());
                    return Err(format!("{name}: columns: item {item} is also in rule {other}"));
                }
                None => {
                    seen.insert(key.clone(), (index, item));
                }
            }
        }
        rules.push(rule);
    }
    Ok(rules)
}

/// Casts one rule's body; reasons are not yet prefixed with its name.
fn rule(name: &str, node: &Node) -> Result<Rule, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err("rule name must use only a-z, 0-9, _ and -".into());
    }
    let fields = match node {
        Node::Map(fields) => fields,
        other => return Err(unexpected("mapping", other)),
    };
    if let Some(field) = fields
        .keys()
        .find(|f| !["columns", "entity", "operator"].contains(&f.as_str()))
    {
        return Err(format!("{field}: unknown field; expected one of {FIELDS}"));
    }
    let field = |key: &str| fields.get(key).map(|(_, node)| &node.value);
    let columns = columns(field("columns").ok_or("columns: required")?).map_err(|r| format!("columns: {r}"))?;
    let entity = entity(field("entity").ok_or("entity: required")?).map_err(|r| format!("entity: {r}"))?;
    let operator = match field("operator") {
        None => None,
        Some(node) => operator(node).map_err(|r| format!("operator: {r}"))?,
    };
    Ok(Rule {
        name: name.to_owned(),
        columns,
        entity,
        operator,
    })
}

/// Casts a rule's `columns` list into folded four-part keys.
fn columns(node: &Node) -> Result<Vec<[String; 4]>, String> {
    let items = match node {
        Node::Seq(items) => items,
        Node::Unsupported(tag) => return Err(env::unsupported_tag(tag)),
        other => return Err(unexpected("sequence", other)),
    };
    if items.is_empty() {
        return Err("must not be empty".into());
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| key(&item.value).map_err(|reason| format!("item {i}: {reason}")))
        .collect()
}

/// Splits one column key into its four folded segments.
fn key(node: &Node) -> Result<[String; 4], String> {
    let text = match node {
        Node::Str(text) => text,
        Node::Unsupported(tag) => return Err(env::unsupported_tag(tag)),
        other => return Err(unexpected("string", other)),
    };
    let segments: Vec<&str> = text.split('.').collect();
    let [catalog, schema, table, column] = segments.as_slice() else {
        return Err(format!(
            "expected 4 dot-separated segments (catalog.schema.table.column), found {}",
            segments.len()
        ));
    };
    let segments = [*catalog, *schema, *table, *column];
    for segment in segments {
        if segment.is_empty() {
            return Err("empty segment".into());
        }
        if segment.contains('*') {
            return Err("* is not supported".into());
        }
        if segment.chars().any(char::is_whitespace) {
            return Err("whitespace in a segment".into());
        }
        if segment.contains(['"', '`']) {
            return Err("quotes are not supported; write the name as the catalog stores it".into());
        }
    }
    Ok(segments.map(ColumnRule::fold))
}

/// Checks a rule's entity name against [`ColumnRule::ENTITIES`].
fn entity(node: &Node) -> Result<&'static str, String> {
    match node {
        Node::Str(text) => ColumnRule::ENTITIES
            .iter()
            .find(|name| **name == text.as_str())
            .copied()
            .ok_or_else(|| "unknown entity type; see the data types page".into()),
        Node::Unsupported(tag) => Err(env::unsupported_tag(tag)),
        other => Err(unexpected("string", other)),
    }
}

/// Casts a rule's operator like `pii.operator`; `null` inherits it.
fn operator(node: &Node) -> Result<Option<PiiOperator>, String> {
    match node {
        Node::Null => Ok(None),
        Node::Str(text) => PiiOperator::CHOICES
            .iter()
            .find(|(name, _)| name == text)
            .map(|&(_, operator)| Some(operator))
            .ok_or_else(|| format!("expected one of {}", env::PII_OPERATOR_NAMES.join(", "))),
        other => Err(unexpected("string", other)),
    }
}

/// Renders the reason for a node of the wrong type.
fn unexpected(expected: &str, found: &Node) -> String {
    match found {
        Node::Unsupported(tag) => env::unsupported_tag(tag),
        other => format!("expected {expected}, found {}", other.kind()),
    }
}
