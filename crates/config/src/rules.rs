//! Casts and checks the `pii.rules` column rules.
//!
//! A `pii.rules` value is a mapping of rule names to rules, or a sequence of
//! unnamed rules; messages refer to an unnamed rule as `rule <index>`.
//! [`cast`] backs [`Kind::Rules`]: it checks every rule a single file's
//! value must meet (rule names, fields, column key shape, known entity and
//! operator, and columns repeated within or across rules, compared
//! case-insensitively). The last file that sets `pii.rules` wins it whole, so
//! these checks run on one file's value. [`flatten`] turns the winning value
//! into one [`ColumnRule`] per listed column, each inheriting `pii.operator`
//! when its rule sets none. [`check`] runs the rules that need other settings
//! on the winning value: `pii.enabled` must be on, column rules are not yet
//! supported on any backend, and each column's catalog segment must fit the
//! backend. Reasons are value-free: they name the rule and the item index,
//! never a column key.
//!
//! [`Kind::Rules`]: crate::env::Kind::Rules

use std::collections::HashMap;

use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::env::{self, Setting, choose, unsupported_tag};
use crate::error::{ConfigError, Diagnostic};
use crate::node::Node;
use crate::pii::{ColumnRule, PiiOperator};

/// The fields a rule accepts, in the order error messages list them.
const FIELDS: [&str; 3] = ["columns", "entity", "operator"];

/// One rule from a `pii.rules` value, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The rule's name; `None` for a rule written in a sequence.
    pub(crate) name: Option<String>,
    /// Each listed column key's four segments, in file order.
    pub(crate) columns: Vec<[String; 4]>,
    /// The entity name, one of [`ColumnRule::ENTITIES`].
    pub(crate) entity: &'static str,
    /// The rule's operator; `None` inherits `pii.operator`.
    pub(crate) operator: Option<PiiOperator>,
}

/// Casts a `pii.rules` mapping or sequence, one [`Rule`] per entry.
///
/// # Errors
///
/// Returns the first problem, prefixed with the rule's name, or with
/// `rule <index>` when the rule is unnamed or its name is invalid.
pub(crate) fn cast(node: &Node) -> Result<Vec<Rule>, String> {
    let entries: Vec<(Option<&str>, &Node)> = match node {
        Node::Map(entries) => entries
            .iter()
            .map(|(name, (_, value))| (Some(name.as_str()), &value.value))
            .collect(),
        Node::Seq(items) => items.iter().map(|item| (None, &item.value)).collect(),
        other => return Err(unexpected(other, "mapping or sequence")),
    };
    let mut rules: Vec<Rule> = Vec::with_capacity(entries.len());
    // Each listed column, folded to lower case once, with its rule and item index.
    let mut seen: HashMap<[String; 4], (usize, usize)> = HashMap::new();
    for (index, (name, value)) in entries.into_iter().enumerate() {
        if name.is_some_and(|name| !valid_name(name)) {
            return Err(format!(
                "rule {index}: name must be non-empty and use only a-z, 0-9, _ and -"
            ));
        }
        let label = label(name, index);
        let rule = rule(name, value).map_err(|reason| format!("{label}: {reason}"))?;
        for (i, column) in rule.columns.iter().enumerate() {
            let folded = column.each_ref().map(|segment| segment.to_lowercase());
            match seen.get(&folded) {
                Some(&(other, first)) if other == index => {
                    return Err(format!("{label}: columns: item {i} repeats item {first}"));
                }
                Some(&(other, _)) => {
                    let other = rules[other].name.clone().unwrap_or_else(|| other.to_string());
                    return Err(format!("{label}: columns: item {i} is also in rule {other}"));
                }
                None => {
                    seen.insert(folded, (index, i));
                }
            }
        }
        rules.push(rule);
    }
    Ok(rules)
}

/// Names a rule in messages: its name, or `rule <index>` when unnamed.
fn label(name: Option<&str>, index: usize) -> String {
    name.map_or_else(|| format!("rule {index}"), str::to_owned)
}

/// Flattens rules into one [`ColumnRule`] per listed column, in file order.
///
/// A rule without its own operator takes `default_operator`, the resolved
/// `pii.operator`.
pub(crate) fn flatten(rules: &[Rule], default_operator: PiiOperator) -> Vec<ColumnRule> {
    rules
        .iter()
        .flat_map(|rule| {
            rule.columns.iter().map(|key| ColumnRule {
                name: rule.name.clone(),
                key: key.clone(),
                entity: rule.entity,
                operator: rule.operator.unwrap_or(default_operator),
            })
        })
        .collect()
}

/// Checks the winning `pii.rules` value against the settings it depends on.
///
/// Reports, in this order: rules set while `pii_enabled` is off; rules set
/// while it is on, since no backend enforces them yet; and each rule whose
/// column keys name a catalog the backend never uses: `def` on
/// `MySQL`/`MariaDB`, `main` on `SQLite`, and `database.name` on `PostgreSQL`
/// when that is set. The catalog is compared case-insensitively. The catalog
/// check always runs.
pub(crate) fn check(rules: &[Rule], pii_enabled: bool, database: &DatabaseConfig) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    let backend = database.backend;
    if !rules.is_empty() {
        // Temporary: drop this gate once a backend enforces column rules.
        errors.push(if pii_enabled {
            Diagnostic::new(ConfigError::PiiRulesUnsupported(backend)).involving(&[env::PII_RULES, env::DB_BACKEND])
        } else {
            Diagnostic::new(ConfigError::PiiRulesWithoutPii).involving(&[env::PII_RULES, env::PII_ENABLE])
        });
    }
    let (catalog, expected) = match backend {
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => ("def", "def"),
        DatabaseBackend::Sqlite => ("main", "main"),
        DatabaseBackend::Postgres => match database.name.as_deref() {
            Some(name) if !name.is_empty() => (name, "database.name"),
            _ => return errors,
        },
    };
    let involved: &[Setting] = if backend == DatabaseBackend::Postgres {
        &[env::PII_RULES, env::DB_BACKEND, env::DB_NAME]
    } else {
        &[env::PII_RULES, env::DB_BACKEND]
    };
    let catalog = catalog.to_lowercase();
    for (index, rule) in rules.iter().enumerate() {
        let items: Vec<String> = rule
            .columns
            .iter()
            .enumerate()
            .filter(|(_, key)| key[0].to_lowercase() != catalog)
            .map(|(i, _)| i.to_string())
            .collect();
        let items = match items.as_slice() {
            [] => continue,
            [item] => format!("item {item}"),
            many => format!("items {}", many.join(", ")),
        };
        errors.push(
            Diagnostic::new(ConfigError::PiiRuleCatalog {
                rule: label(rule.name.as_deref(), index),
                items,
                expected,
                backend,
            })
            .involving(involved),
        );
    }
    errors
}

/// Returns `true` when `name` is non-empty and only `a-z`, `0-9`, `_`, `-`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Casts one rule's body.
fn rule(name: Option<&str>, node: &Node) -> Result<Rule, String> {
    let Node::Map(entries) = node else {
        return Err(unexpected(node, "mapping"));
    };
    if let Some(key) = entries.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(format!("{key}: unknown field; expected one of {}", FIELDS.join(", ")));
    }
    let field = |key: &str| entries.get(key).map(|(_, value)| &value.value);
    let columns = match field("columns") {
        None | Some(Node::Null) => return Err("columns: required".into()),
        Some(node) => columns(node).map_err(|reason| format!("columns: {reason}"))?,
    };
    let entity = match field("entity") {
        None | Some(Node::Null) => return Err("entity: required".into()),
        Some(Node::Str(text)) => choose(&ColumnRule::ENTITIES, text).map_err(|_| "entity: unknown entity type")?,
        Some(other) => return Err(format!("entity: {}", unexpected(other, "string"))),
    };
    let operator = match field("operator") {
        None | Some(Node::Null) => None,
        Some(Node::Str(text)) => Some(
            PiiOperator::CHOICES
                .iter()
                .find(|(name, _)| name == text)
                .map(|&(_, operator)| operator)
                .ok_or_else(|| {
                    let names: Vec<&str> = PiiOperator::CHOICES.iter().map(|&(name, _)| name).collect();
                    format!("operator: expected one of {}", names.join(", "))
                })?,
        ),
        Some(other) => return Err(format!("operator: {}", unexpected(other, "string"))),
    };
    Ok(Rule {
        name: name.map(str::to_owned),
        columns,
        entity,
        operator,
    })
}

/// Casts a rule's `columns` sequence into four-segment keys.
fn columns(node: &Node) -> Result<Vec<[String; 4]>, String> {
    let Node::Seq(items) = node else {
        return Err(unexpected(node, "sequence"));
    };
    if items.is_empty() {
        return Err("must not be empty".into());
    }
    let mut columns: Vec<[String; 4]> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let column = match &item.value {
            Node::Str(text) => key(text),
            other => Err(unexpected(other, "string")),
        }
        .map_err(|reason| format!("item {i}: {reason}"))?;
        columns.push(column);
    }
    Ok(columns)
}

/// Splits a column key into its `catalog.schema.table.column` segments.
fn key(text: &str) -> Result<[String; 4], String> {
    let segments: Vec<&str> = text.split('.').collect();
    let Ok(key) = <[&str; 4]>::try_from(segments.as_slice()) else {
        return Err(format!(
            "expected 4 dot-separated segments (catalog.schema.table.column), found {}",
            segments.len()
        ));
    };
    for segment in key {
        if segment.is_empty() {
            return Err("empty segment".into());
        }
        if segment.contains('*') {
            return Err("* is not supported".into());
        }
        if segment.chars().any(char::is_whitespace) {
            return Err("whitespace is not allowed".into());
        }
    }
    Ok(key.map(str::to_owned))
}

/// Renders a value-free reason for a node that is not the `expected` type.
fn unexpected(node: &Node, expected: &str) -> String {
    match node {
        Node::Unsupported(tag) => unsupported_tag(tag),
        other => format!("expected {expected}, found {}", other.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cast_yaml(yaml: &str) -> Result<Vec<Rule>, String> {
        cast(&crate::node::parse(yaml).expect("valid yaml").value)
    }

    fn key_of(text: &str) -> [String; 4] {
        key(text).expect("valid key")
    }

    #[test]
    fn rules_cast_in_file_order() {
        let rules = cast_yaml(
            "emails:\n  columns: [def.shop.users.email, def.shop.orders.email]\n  entity: EMAIL_ADDRESS\n\
             names:\n  columns: [def.shop.users.name]\n  entity: PERSON\n  operator: mask\n",
        )
        .expect("valid rules");
        assert_eq!(
            rules,
            [
                Rule {
                    name: Some("emails".into()),
                    columns: vec![key_of("def.shop.users.email"), key_of("def.shop.orders.email")],
                    entity: "EMAIL_ADDRESS",
                    operator: None,
                },
                Rule {
                    name: Some("names".into()),
                    columns: vec![key_of("def.shop.users.name")],
                    entity: "PERSON",
                    operator: Some(PiiOperator::Mask),
                },
            ]
        );
        assert_eq!(
            cast_yaml("a: 1\n").map(|_| ()),
            Err("a: expected mapping, found integer".into())
        );
    }

    /// Runs [`check`] on `yaml`'s rules and returns the messages.
    fn checked(yaml: &str, pii_enabled: bool, database: &DatabaseConfig) -> Vec<String> {
        let rules = cast_yaml(yaml).expect("valid rules");
        check(&rules, pii_enabled, database)
            .iter()
            .map(|d| d.error.to_string())
            .collect()
    }

    /// One rule `r` whose columns use `catalogs`, in order.
    fn with_catalogs(catalogs: &[&str]) -> String {
        let columns: Vec<String> = catalogs
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c}.s.t.c{i}"))
            .collect();
        format!("r:\n  columns: [{}]\n  entity: URL\n", columns.join(", "))
    }

    fn database(backend: DatabaseBackend, name: Option<&str>) -> DatabaseConfig {
        DatabaseConfig {
            name: name.map(Into::into),
            ..crate::database::tests::db_config(backend)
        }
    }

    #[test]
    fn rule_catalog_must_fit_the_backend() {
        let fits = [
            (DatabaseBackend::Mysql, None, &["def", "DEF"][..]),
            (DatabaseBackend::Mariadb, None, &["def"]),
            (DatabaseBackend::Sqlite, Some("x.db"), &["main", "Main"]),
            (DatabaseBackend::Postgres, Some("App"), &["app"]),
            (DatabaseBackend::Postgres, None, &["anything"]),
            (DatabaseBackend::Postgres, Some(""), &["anything"]),
        ];
        for (backend, name, catalogs) in fits {
            assert_eq!(
                checked(&with_catalogs(catalogs), true, &database(backend, name)),
                [gate(backend)],
                "{backend}"
            );
        }
        let cases = [
            (
                DatabaseBackend::Mysql,
                None,
                &["def", "shop"][..],
                "pii.rules: r: columns item 1: the catalog segment must be def on mysql",
            ),
            (
                DatabaseBackend::Sqlite,
                Some("x.db"),
                &["temp", "main", "x"],
                "pii.rules: r: columns items 0, 2: the catalog segment must be main on sqlite",
            ),
            (
                DatabaseBackend::Postgres,
                Some("app"),
                &["other"],
                "pii.rules: r: columns item 0: the catalog segment must be database.name on postgres",
            ),
        ];
        for (backend, name, catalogs, message) in cases {
            assert_eq!(
                checked(&with_catalogs(catalogs), true, &database(backend, name)),
                [gate(backend), message.to_owned()]
            );
        }
    }

    /// The temporary backend gate's message on `backend`.
    fn gate(backend: DatabaseBackend) -> String {
        format!("pii.rules: column rules are not yet supported on {backend}")
    }

    #[test]
    fn column_rules_are_not_yet_supported_on_any_backend() {
        for (backend, name, catalog) in [
            (DatabaseBackend::Mysql, None, "def"),
            (DatabaseBackend::Mariadb, None, "def"),
            (DatabaseBackend::Postgres, Some("app"), "app"),
            (DatabaseBackend::Sqlite, Some("x.db"), "main"),
        ] {
            let database = database(backend, name);
            assert_eq!(
                checked(&with_catalogs(&[catalog]), true, &database),
                [gate(backend)],
                "{backend}"
            );
            assert!(check(&[], true, &database).is_empty(), "{backend}");
        }
    }

    #[test]
    fn flatten_yields_one_column_rule_per_listed_column() {
        let rules = cast_yaml(
            "emails:\n  columns: [def.shop.users.email, def.shop.orders.Email]\n  entity: EMAIL_ADDRESS\n\
             names:\n  columns: [def.shop.users.name]\n  entity: PERSON\n  operator: redact\n",
        )
        .expect("valid rules");
        let flat: Vec<_> = flatten(&rules, PiiOperator::Mask)
            .into_iter()
            .map(|rule| {
                (
                    rule.name.expect("named"),
                    rule.key.join("."),
                    rule.entity,
                    rule.operator,
                )
            })
            .collect();
        assert_eq!(
            flat,
            [
                (
                    "emails".to_owned(),
                    "def.shop.users.email".to_owned(),
                    "EMAIL_ADDRESS",
                    PiiOperator::Mask
                ),
                (
                    "emails".to_owned(),
                    "def.shop.orders.Email".to_owned(),
                    "EMAIL_ADDRESS",
                    PiiOperator::Mask
                ),
                (
                    "names".to_owned(),
                    "def.shop.users.name".to_owned(),
                    "PERSON",
                    PiiOperator::Redact
                ),
            ]
        );
        assert!(flatten(&[], PiiOperator::Hash).is_empty());
    }

    #[test]
    fn rules_cast_every_operator_name() {
        let magento = cast_yaml(
            "emails:\n  columns:\n    - def.magento.customer_entity.email\n    - def.magento.sales_order.customer_email\n  \
             entity: EMAIL_ADDRESS\n\
             names:\n  columns:\n    - def.magento.customer_entity.firstname\n    - def.magento.customer_entity.lastname\n  \
             entity: PERSON\n  operator: replace\n\
             phones:\n  columns: [def.magento.sales_order_address.telephone]\n  entity: PHONE_NUMBER\n  operator: mask\n\
             cards:\n  columns: [def.magento.sales_order_payment.cc_number_enc]\n  entity: CREDIT_CARD\n  operator: redact\n\
             tokens:\n  columns: [def.magento.oauth_token.secret]\n  entity: API_KEY\n  operator: hash\n",
        )
        .expect("valid rules");
        let operators: Vec<_> = magento.iter().map(|rule| rule.operator).collect();
        assert_eq!(
            operators,
            [
                None,
                Some(PiiOperator::Replace),
                Some(PiiOperator::Mask),
                Some(PiiOperator::Redact),
                Some(PiiOperator::Hash),
            ]
        );
    }

    #[test]
    fn rules_require_pii_enabled_and_every_check_runs() {
        let mysql = database(DatabaseBackend::Mysql, None);
        assert_eq!(
            checked(&with_catalogs(&["def"]), false, &mysql),
            ["pii.rules requires pii.enabled"]
        );
        assert_eq!(
            checked(&with_catalogs(&["shop"]), false, &mysql),
            [
                "pii.rules requires pii.enabled",
                "pii.rules: r: columns item 0: the catalog segment must be def on mysql",
            ]
        );
        assert!(check(&[], false, &mysql).is_empty());
        assert!(check(&[], true, &mysql).is_empty());
    }

    #[test]
    fn unnamed_rules_cast_from_a_sequence() {
        let rules = cast_yaml(
            "- columns: [def.shop.users.email]\n  entity: EMAIL_ADDRESS\n\
             - columns: [def.shop.users.name]\n  entity: PERSON\n  operator: mask\n",
        )
        .expect("valid rules");
        assert_eq!(
            rules,
            [
                Rule {
                    name: None,
                    columns: vec![key_of("def.shop.users.email")],
                    entity: "EMAIL_ADDRESS",
                    operator: None,
                },
                Rule {
                    name: None,
                    columns: vec![key_of("def.shop.users.name")],
                    entity: "PERSON",
                    operator: Some(PiiOperator::Mask),
                },
            ]
        );
        assert_eq!(flatten(&rules, PiiOperator::Hash)[0].name, None);
        assert!(cast_yaml("[]\n").expect("empty").is_empty());
    }

    #[test]
    fn unnamed_rules_are_named_by_index_in_messages() {
        let body = "  entity: EMAIL_ADDRESS\n";
        let cases = [
            (
                "- columns: [a.b.c.d]\n".to_owned() + body + "- 1\n",
                "rule 1: expected mapping, found integer",
            ),
            (
                "- columns: [a.b.c]\n".to_owned() + body,
                "rule 0: columns: item 0: expected 4 dot-separated segments (catalog.schema.table.column), found 3",
            ),
            (
                "- columns: [a.b.c.d]\n  name: x\n".to_owned() + body,
                "rule 0: name: unknown field; expected one of columns, entity, operator",
            ),
            (
                "- columns: [a.b.c.d, A.B.C.D]\n".to_owned() + body,
                "rule 0: columns: item 1 repeats item 0",
            ),
            (
                "- columns: [a.b.c.d]\n".to_owned() + body + "- columns: [a.B.c.d]\n" + body,
                "rule 1: columns: item 0 is also in rule 0",
            ),
            ("\"\"\n".to_owned(), "expected mapping or sequence, found string"),
        ];
        for (yaml, reason) in cases {
            assert_eq!(cast_yaml(&yaml).map(|_| ()), Err(reason.into()), "{yaml}");
        }
        assert_eq!(
            checked(
                "- columns: [def.s.t.c]\n  entity: URL\n- columns: [shop.s.t.c]\n  entity: URL\n",
                true,
                &database(DatabaseBackend::Mysql, None)
            ),
            [
                gate(DatabaseBackend::Mysql),
                "pii.rules: rule 1: columns item 0: the catalog segment must be def on mysql".to_owned(),
            ]
        );
    }

    #[test]
    fn column_case_is_kept_as_written() {
        let rules = cast_yaml("a:\n  columns: [Def.Shop.Users.Email]\n  entity: URL\n  operator: ~\n").expect("valid");
        assert_eq!(rules[0].columns, [key_of("Def.Shop.Users.Email")]);
        assert_eq!(rules[0].operator, None, "null inherits pii.operator");
    }

    #[test]
    fn problems_name_the_rule_and_item_without_the_value() {
        let body = "  entity: EMAIL_ADDRESS\n";
        let cases = [
            (
                "ok:\n  columns: [a.b.c.d]\n".to_owned() + body + "\"a b\":\n  columns: [a.b.c.e]\n" + body,
                "rule 1: name must be non-empty and use only a-z, 0-9, _ and -",
            ),
            ("r: [a.b.c.d]\n".to_owned(), "r: expected mapping, found sequence"),
            ("r:\n".to_owned(), "r: expected mapping, found null"),
            ("r: !x {}\n".to_owned(), "r: YAML tag !x is not supported"),
            (
                "r:\n  columns: [a.b.c.d]\n  match: x\n".to_owned() + body,
                "r: match: unknown field; expected one of columns, entity, operator",
            ),
            ("r:\n".to_owned() + body, "r: columns: required"),
            ("r:\n  columns: [a.b.c.d]\n".to_owned(), "r: entity: required"),
            ("r:\n  columns: []\n".to_owned() + body, "r: columns: must not be empty"),
            (
                "r:\n  columns: a.b.c.d\n".to_owned() + body,
                "r: columns: expected sequence, found string",
            ),
            (
                "r:\n  columns: [a.b.c.d, 1]\n".to_owned() + body,
                "r: columns: item 1: expected string, found integer",
            ),
            (
                "r:\n  columns: [a.b.c]\n".to_owned() + body,
                "r: columns: item 0: expected 4 dot-separated segments (catalog.schema.table.column), found 3",
            ),
            (
                "r:\n  columns: [a..c.d]\n".to_owned() + body,
                "r: columns: item 0: empty segment",
            ),
            (
                "r:\n  columns: [a.b.*.d]\n".to_owned() + body,
                "r: columns: item 0: * is not supported",
            ),
            (
                "r:\n  columns: [\"a.b.c. d\"]\n".to_owned() + body,
                "r: columns: item 0: whitespace is not allowed",
            ),
            (
                "r:\n  columns: [a.b.c.d]\n  entity: EMAIL\n".to_owned(),
                "r: entity: unknown entity type",
            ),
            (
                "r:\n  columns: [a.b.c.d]\n  entity: [EMAIL_ADDRESS]\n".to_owned(),
                "r: entity: expected string, found sequence",
            ),
            (
                "r:\n  columns: [a.b.c.d]\n  operator: scramble\n".to_owned() + body,
                "r: operator: expected one of replace, mask, redact, hash",
            ),
            (
                "r:\n  columns: [a.b.c.d, A.B.C.D]\n".to_owned() + body,
                "r: columns: item 1 repeats item 0",
            ),
            (
                "r:\n  columns: [a.b.c.d]\n".to_owned() + body + "s:\n  columns: [x.y.z.w, a.B.c.d]\n" + body,
                "s: columns: item 1 is also in rule r",
            ),
        ];
        for (yaml, reason) in cases {
            assert_eq!(cast_yaml(&yaml).map(|_| ()), Err(reason.into()), "{yaml}");
        }
    }
}
