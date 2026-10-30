//! `${}` interpolation over every string value of the YAML tree.
//!
//! Keys are never touched,
//! sequence items are substituted one by one, and errors are collected for
//! every value and sorted by key path. Casting to each setting's type
//! happens later, when the loader resolves settings.

use crate::node::{Node, SpannedNode};
use crate::template::{self, Lookup};

/// Messages from one interpolation pass, each `line:column: path: text`.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Unset variables read without a default.
    pub(crate) warnings: Vec<String>,
    /// Malformed templates and failed `?` / `:?` references.
    pub(crate) errors: Vec<String>,
}

/// Substitutes every string value in `root` in place.
pub(crate) fn interpolate(root: &mut SpannedNode, lookup: Lookup<'_>) -> Outcome {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    walk(root, "", lookup, &mut errors, &mut warnings);
    errors.sort_by(|a: &(String, String), b| a.0.cmp(&b.0));
    Outcome {
        warnings,
        errors: errors.into_iter().map(|(_, message)| message).collect(),
    }
}

fn walk(
    node: &mut SpannedNode,
    path: &str,
    lookup: Lookup<'_>,
    errors: &mut Vec<(String, String)>,
    warnings: &mut Vec<String>,
) {
    let at = format!("{}:{}", node.referenced.line(), node.referenced.column());
    match &mut node.value {
        Node::Str(text) => {
            let mut unset = Vec::new();
            match template::substitute(text, lookup, &mut unset) {
                Ok(value) => *text = value,
                Err(error) => errors.push((path.to_owned(), format!("{at}: {path}: {error}"))),
            }
            warnings.extend(
                unset.into_iter().map(|name| {
                    format!("{at}: {path}: the \"{name}\" variable is not set; defaulting to a blank string")
                }),
            );
        }
        Node::Seq(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                walk(item, &format!("{path}[{i}]"), lookup, errors, warnings);
            }
        }
        Node::Map(entries) => {
            for (key, value) in entries.iter_mut() {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                walk(value, &child, lookup, errors, warnings);
            }
        }
        Node::Null | Node::Bool(_) | Node::Int(_) | Node::UInt(_) | Node::Float(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node;

    fn env(name: &str) -> Option<String> {
        match name {
            "HOST" => Some("db.internal".into()),
            "PORT" => Some("5432".into()),
            _ => None,
        }
    }

    fn get<'a>(root: &'a SpannedNode, path: &[&str]) -> &'a Node {
        path.iter().fold(&root.value, |node, key| match node {
            Node::Map(map) => &map[*key].value,
            other => panic!("not a mapping: {other:?}"),
        })
    }

    #[test]
    fn substitutes_values_in_sections_and_sequences() {
        let mut root =
            node::parse("database:\n  host: ${HOST}\n  port: ${PORT}\nhttp:\n  allowed_hosts: [\"$HOST\", x]\n")
                .expect("valid yaml");
        let outcome = interpolate(&mut root, &env);
        assert!(outcome.errors.is_empty() && outcome.warnings.is_empty(), "{outcome:?}");
        assert_eq!(get(&root, &["database", "host"]), &Node::Str("db.internal".into()));
        assert_eq!(get(&root, &["database", "port"]), &Node::Str("5432".into()));
        let Node::Seq(items) = get(&root, &["http", "allowed_hosts"]) else {
            panic!("sequence");
        };
        assert_eq!(items[0].value, Node::Str("db.internal".into()));
    }

    #[test]
    fn keys_are_never_substituted() {
        let mut root = node::parse("${HOST}: 1\n").expect("valid yaml");
        interpolate(&mut root, &env);
        assert_eq!(get(&root, &["${HOST}"]), &Node::Int(1));
    }

    #[test]
    fn errors_are_collected_sorted_by_path_without_values() {
        let mut root =
            node::parse("ssh:\n  host: ${A:?need A}\ndatabase:\n  password: \"s3cret${\"\n").expect("valid yaml");
        let outcome = interpolate(&mut root, &env);
        assert_eq!(
            outcome.errors,
            [
                "4:13: database.password: invalid interpolation format; escape a literal $ as $$",
                "2:9: ssh.host: required variable A is missing a value: need A",
            ]
        );
        assert!(!outcome.errors.concat().contains("s3cret"));
    }

    #[test]
    fn unset_variables_warn_and_become_empty() {
        let mut root = node::parse("database:\n  user: ${NOPE}\n").expect("valid yaml");
        let outcome = interpolate(&mut root, &env);
        assert_eq!(get(&root, &["database", "user"]), &Node::Str(String::new()));
        assert_eq!(
            outcome.warnings,
            ["2:9: database.user: the \"NOPE\" variable is not set; defaulting to a blank string"]
        );
    }
}
