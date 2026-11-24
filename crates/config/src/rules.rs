//! Casts one config file's `privacy.rules` map and resolves the winning rules.
//!
//! `privacy.rules` is a map of named rules; each lists column keys of two to
//! four segments (`[[catalog.]schema.]table.column`) with an entity and an
//! optional operator. [`cast`] checks a file's value whole: names, fields,
//! key shape, the entity, and the operator. [`resolve`] turns the winning
//! rules into [`privacy::ColumnRule`]s once the backend is known: it fills each short
//! key's leading segments, checks written catalogs, and finds repeats.
//! Reasons are value-free: they name the rule and the item index, never a
//! column key.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_saphyr::{Location, Spanned};

use crate::choices;
use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};
use crate::node::Node;
use dbmcp_types::privacy::Entity;
use dbmcp_types::{database, privacy};

/// One named rule as a config file wrote it, with folded keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The rule's name.
    pub name: String,
    /// Each listed column's key, as written.
    pub columns: Vec<Key>,
    /// The entity the columns hold.
    pub entity: Entity,
    /// The rule's operator; `None` inherits `privacy.operator`.
    pub operator: Option<privacy::Operator>,
}

/// A column key as written, its segments folded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Key {
    /// `table.column`.
    Table([String; 2]),
    /// `schema.table.column`.
    Schema([String; 3]),
    /// `catalog.schema.table.column`.
    Full([String; 4]),
}

/// The fields a rule may set.
const FIELDS: &str = "columns, entity, operator";

/// Casts a `privacy.rules` mapping into its rules, in file order.
///
/// # Errors
///
/// Returns the first value-free reason, prefixed with the rule's name, or
/// with `rule <index>` when the name itself is invalid (a misplaced column
/// key must not be echoed): a bad name, a rule that is not a mapping, an unknown or missing field, an
/// empty or malformed column list, or an unknown entity or operator.
pub(crate) fn cast(entries: &IndexMap<String, (Location, Spanned<Node>)>) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::with_capacity(entries.len());
    for (index, (name, (_, node))) in entries.iter().enumerate() {
        if !valid_name(name) {
            return Err(format!("rule {index}: rule name must use only a-z, 0-9, _ and -"));
        }
        rules.push(rule(name, &node.value).map_err(|reason| format!("{name}: {reason}"))?);
    }
    Ok(rules)
}

/// Resolves rules into one [`privacy::ColumnRule`] per listed column, keys filled.
///
/// Segments drop from the left, as in the engine's own qualified names:
/// `MySQL`/`MariaDB` fill catalog `def` and schema `database.name`;
/// `PostgreSQL` fills catalog `database.name` and schema `public`; `SQLite`
/// fills `main.main`. A rule's own operator wins over `operator`.
///
/// # Errors
///
/// Returns every value-free problem: per rule, the items whose missing
/// segment needs an unset `database.name`, and the full keys whose catalog
/// does not fit the backend; then each column listed again.
pub(crate) fn resolve(
    rules: &[Rule],
    database: &database::Config,
    operator: privacy::Operator,
) -> Result<Vec<privacy::ColumnRule>, Vec<Diagnostic>> {
    let backend = database.backend;
    let pinned = database.name.as_deref().map(privacy::ColumnRule::fold);
    let (catalog, schema) = match backend {
        database::Backend::Mysql | database::Backend::Mariadb => (Some("def".to_owned()), pinned),
        database::Backend::Sqlite => (Some("main".to_owned()), Some("main".to_owned())),
        database::Backend::Postgres => (pinned, Some("public".to_owned())),
    };
    let list = |items: &[usize]| {
        items
            .iter()
            .map(|item| format!("item {item}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut resolved = Vec::new();
    let mut errors = Vec::new();
    for rule in rules {
        let mut incomplete = Vec::new();
        let mut foreign = Vec::new();
        for (item, key) in rule.columns.iter().enumerate() {
            let full = match key {
                Key::Full(key) => {
                    if catalog.as_ref().is_some_and(|catalog| *catalog != key[0]) {
                        foreign.push(item);
                    }
                    Some(key.clone())
                }
                Key::Schema([schema, table, column]) => catalog
                    .clone()
                    .map(|catalog| [catalog, schema.clone(), table.clone(), column.clone()]),
                Key::Table([table, column]) => catalog
                    .clone()
                    .zip(schema.clone())
                    .map(|(catalog, schema)| [catalog, schema, table.clone(), column.clone()]),
            };
            let Some(key) = full else {
                incomplete.push(item);
                continue;
            };
            resolved.push(privacy::ColumnRule {
                name: rule.name.clone(),
                item,
                key,
                entity: rule.entity,
                operator: rule.operator.unwrap_or(operator),
            });
        }
        if !incomplete.is_empty() {
            errors.push(
                Diagnostic::new(ConfigError::PrivacyRuleNeedsDatabase {
                    rule: rule.name.clone(),
                    items: list(&incomplete),
                    backend,
                })
                .involving(&[env::PRIVACY_RULES, env::DB_BACKEND])
                .missing(&[env::DB_NAME]),
            );
        }
        if !foreign.is_empty() {
            errors.push(
                Diagnostic::new(ConfigError::PrivacyRuleCatalog {
                    rule: rule.name.clone(),
                    items: list(&foreign),
                    // PostgreSQL's catalog comes from config; name the setting, not its value.
                    expected: match backend {
                        database::Backend::Postgres => "database.name".to_owned(),
                        _ => catalog.clone().unwrap_or_default(),
                    },
                    backend,
                })
                .involving(match backend {
                    database::Backend::Postgres => &[env::PRIVACY_RULES, env::DB_BACKEND, env::DB_NAME],
                    _ => &[env::PRIVACY_RULES, env::DB_BACKEND],
                }),
            );
        }
    }
    errors.extend(repeats(&resolved));
    into_result(errors).map(|()| resolved)
}

/// Reports each column listed again, naming the item or rule that listed it first.
fn repeats(columns: &[privacy::ColumnRule]) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    let mut seen: HashMap<&[String; 4], &privacy::ColumnRule> = HashMap::new();
    for column in columns {
        let Some(first) = seen.get(&column.key) else {
            seen.insert(&column.key, column);
            continue;
        };
        let error = if first.name == column.name {
            ConfigError::PrivacyRuleRepeat {
                rule: column.name.clone(),
                item: column.item,
                first: first.item,
            }
        } else {
            ConfigError::PrivacyRuleShared {
                rule: column.name.clone(),
                item: column.item,
                other: first.name.clone(),
            }
        };
        errors.push(Diagnostic::new(error).involving(&[env::PRIVACY_RULES]));
    }
    errors
}

/// Returns whether a rule name is non-empty `a-z`, `0-9`, `_`, and `-`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Casts one rule's body; reasons are not yet prefixed with its name.
fn rule(name: &str, node: &Node) -> Result<Rule, String> {
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
    // A field set to null counts as missing.
    let field = |key: &str| {
        fields
            .get(key)
            .map(|(_, node)| &node.value)
            .filter(|node| !matches!(node, Node::Null))
    };
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

/// Casts a rule's `columns` list into folded keys.
fn columns(node: &Node) -> Result<Vec<Key>, String> {
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

/// Splits one column key into its two to four folded segments.
fn key(node: &Node) -> Result<Key, String> {
    let text = match node {
        Node::Str(text) => text,
        Node::Unsupported(tag) => return Err(env::unsupported_tag(tag)),
        other => return Err(unexpected("string", other)),
    };
    let segments: Vec<&str> = text.split('.').collect();
    let key = match segments.as_slice() {
        [table, column] => Key::Table([*table, *column].map(privacy::ColumnRule::fold)),
        [schema, table, column] => Key::Schema([*schema, *table, *column].map(privacy::ColumnRule::fold)),
        [catalog, schema, table, column] => {
            Key::Full([*catalog, *schema, *table, *column].map(privacy::ColumnRule::fold))
        }
        _ => {
            return Err(format!(
                "expected 2 to 4 dot-separated segments ([[catalog.]schema.]table.column), found {}",
                segments.len()
            ));
        }
    };
    for &segment in &segments {
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
    Ok(key)
}

/// Parses a rule's entity name.
fn entity(node: &Node) -> Result<Entity, String> {
    match node {
        Node::Str(text) => text
            .parse()
            .map_err(|_| "unknown entity type; see the data types page".into()),
        Node::Unsupported(tag) => Err(env::unsupported_tag(tag)),
        other => Err(unexpected("string", other)),
    }
}

/// Casts a rule's operator like `privacy.operator`; `null` inherits it.
fn operator(node: &Node) -> Result<Option<privacy::Operator>, String> {
    match node {
        Node::Null => Ok(None),
        Node::Str(text) => choices::PRIVACY_OPERATOR
            .iter()
            .find(|(name, _)| name == text)
            .map(|&(_, operator)| Some(operator))
            .ok_or_else(|| format!("expected one of {}", env::PRIVACY_OPERATOR_NAMES.join(", "))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::database::tests::db_config;

    /// Resolves `rules` of `(name, keys)` on `backend`; returns full keys or error messages.
    fn resolved(
        backend: database::Backend,
        name: Option<&str>,
        rules: &[(&str, &[&str])],
    ) -> Result<Vec<String>, Vec<String>> {
        let rules: Vec<Rule> = rules
            .iter()
            .map(|(rule, keys)| Rule {
                name: (*rule).to_owned(),
                columns: keys
                    .iter()
                    .map(|text| key(&Node::Str((*text).to_owned())).expect("valid key"))
                    .collect(),
                entity: Entity::Url,
                operator: None,
            })
            .collect();
        let database = database::Config {
            name: name.map(str::to_owned),
            ..db_config(backend)
        };
        resolve(&rules, &database, privacy::Operator::Hash)
            .map(|columns| columns.iter().map(|column| column.key.join(".")).collect())
            .map_err(|errors| errors.iter().map(|error| error.error.to_string()).collect())
    }

    /// Resolves one `emails` rule over `keys`.
    fn emails(backend: database::Backend, name: Option<&str>, keys: &[&str]) -> Result<Vec<String>, Vec<String>> {
        resolved(backend, name, &[("emails", keys)])
    }

    #[test]
    fn short_keys_fill_leading_segments_per_backend() {
        use database::Backend::{Mariadb, Mysql, Postgres, Sqlite};
        let cases = [
            (Mysql, None, "Shop.Users.Email", "def.shop.users.email"),
            (Mysql, Some("Shop"), "users.email", "def.shop.users.email"),
            (Mysql, Some("shop"), "other.users.email", "def.other.users.email"),
            (Mariadb, Some("shop"), "users.email", "def.shop.users.email"),
            (Postgres, Some("Shop"), "users.email", "shop.public.users.email"),
            (Postgres, Some("shop"), "audit.users.email", "shop.audit.users.email"),
            (Postgres, None, "shop.audit.users.email", "shop.audit.users.email"),
            (Sqlite, Some("app.db"), "users.email", "main.main.users.email"),
            (Sqlite, Some("app.db"), "aux.users.email", "main.aux.users.email"),
        ];
        for (backend, name, key, expanded) in cases {
            assert_eq!(
                emails(backend, name, &[key]),
                Ok(vec![expanded.to_owned()]),
                "{backend} {key}"
            );
        }
    }

    #[test]
    fn short_keys_without_their_database_need_database_name() {
        for (backend, keys, items) in [
            (
                database::Backend::Mysql,
                &["users.email", "shop.users.email"][..],
                "item 0",
            ),
            (database::Backend::Mariadb, &["users.email"][..], "item 0"),
            (
                database::Backend::Postgres,
                &["users.email", "public.users.email", "shop.public.users.email"][..],
                "item 0, item 1",
            ),
        ] {
            assert_eq!(
                emails(backend, None, keys),
                Err(vec![format!(
                    "privacy.rules: emails: columns: {items}: a key without its database requires database.name on {backend}"
                )]),
                "{backend}"
            );
        }
    }

    #[test]
    fn full_key_catalogs_must_fit_the_backend() {
        use database::Backend::{Mariadb, Mysql, Postgres, Sqlite};
        assert!(emails(Mysql, None, &["DEF.s.t.c"]).is_ok());
        assert_eq!(
            emails(Mariadb, None, &["def.s.t.a", "main.s.t.b", "x.s.t.c"]),
            Err(vec![
                "privacy.rules: emails: columns: item 1, item 2: catalog segment must be def on mariadb".to_owned()
            ])
        );
        assert!(emails(Sqlite, Some("app.db"), &["Main.s.t.c"]).is_ok());
        assert_eq!(
            emails(Sqlite, Some("app.db"), &["def.s.t.c"]),
            Err(vec![
                "privacy.rules: emails: columns: item 0: catalog segment must be main on sqlite".to_owned()
            ])
        );
        assert!(emails(Postgres, Some("Shop"), &["shop.public.t.c"]).is_ok());
        assert_eq!(
            emails(Postgres, Some("shop"), &["other.public.t.c"]),
            Err(vec![
                "privacy.rules: emails: columns: item 0: catalog segment must be database.name on postgres".to_owned()
            ])
        );
        assert!(emails(Postgres, None, &["anything.public.t.c"]).is_ok());
    }

    #[test]
    fn a_column_listed_twice_is_found_after_expansion() {
        assert_eq!(
            emails(
                database::Backend::Mysql,
                Some("shop"),
                &["users.email", "def.shop.users.email", "SHOP.Users.Email"]
            ),
            Err(vec![
                "privacy.rules: emails: columns: item 1 repeats item 0".to_owned(),
                "privacy.rules: emails: columns: item 2 repeats item 0".to_owned(),
            ])
        );
        assert_eq!(
            resolved(
                database::Backend::Sqlite,
                Some("app.db"),
                &[("emails", &["users.email"]), ("cards", &["main.main.users.email"])]
            ),
            Err(vec![
                "privacy.rules: cards: columns: item 0 is also in rule emails".to_owned()
            ])
        );
    }
}
