//! The config schema: the JSON Schema every config file is validated against.
//!
//! The schema is hand-written in `assets/dbmcp.schema.json` and embedded in
//! the binary. Editors use all of it; the loader uses only its structure.
//! [`validate`] strips the value keywords (`type`, `enum`, `minimum`,
//! `maximum`, `items`) from every setting, then checks the file's keys and
//! the shape of the root and each section. Setting values are checked by
//! their [`Kind`] instead. Errors are rendered here, value-free; `boon`'s own
//! messages echo values and are never shown.
//!
//! [`Kind`]: crate::env::Kind

use std::collections::HashSet;
use std::path::Path;

use boon::{Compiler, ErrorKind, InstanceToken, SchemaIndex, Schemas, Type, Types, ValidationError};
use serde_json::{Map, Value};

use crate::env::SETTINGS;
use crate::node::{Node, SpannedNode};
use crate::settings::InputError;
use crate::source::Source;

/// The schema's `$id`, also where editors fetch it from.
pub(crate) const URL: &str =
    "https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json";

/// The schema text.
pub(crate) const SCHEMA: &str = include_str!("../assets/dbmcp.schema.json");

/// Keywords that check a setting's value; [`Kind`](crate::env::Kind) owns these rules.
const VALUE_KEYWORDS: [&str; 5] = ["type", "enum", "minimum", "maximum", "items"];

/// Validates the structure of the config file at `file` against the config schema.
///
/// Returns one error per unknown key, at the key, and per root or section
/// that is not a mapping or null, at its value. Empty when valid.
///
/// # Panics
///
/// Panics when the embedded schema does not compile, which its tests rule out.
pub(crate) fn validate(root: &SpannedNode, file: &Path) -> Vec<InputError> {
    let (schemas, index) = compile();
    let mut errors = Vec::new();
    if let Err(error) = schemas.validate(&to_json(&root.value), index) {
        collect(&error, root, file, &mut errors);
    }
    errors
}

/// Compiles the embedded schema without its value keywords.
fn compile() -> (Schemas, SchemaIndex) {
    let mut document: Value = serde_json::from_str(SCHEMA).expect("embedded schema is valid JSON");
    strip_values(&mut document);
    let mut compiler = Compiler::new();
    compiler.add_resource(URL, document).expect("schema URL is absolute");
    let mut schemas = Schemas::new();
    let index = compiler.compile(URL, &mut schemas).expect("embedded schema compiles");
    (schemas, index)
}

/// Removes [`VALUE_KEYWORDS`] from every leaf property: one without `properties`.
fn strip_values(schema: &mut Value) {
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return;
    };
    for property in properties.values_mut() {
        if property.get("properties").is_some() {
            strip_values(property);
        } else if let Some(property) = property.as_object_mut() {
            for keyword in VALUE_KEYWORDS {
                property.remove(keyword);
            }
        }
    }
}

/// Converts the YAML tree to JSON for validation.
///
/// Only the shape of the root and sections is checked, so any number
/// stands in for a float; errors name the YAML node's own type.
fn to_json(node: &Node) -> Value {
    match node {
        Node::Null => Value::Null,
        Node::Bool(value) => Value::Bool(*value),
        Node::Int(value) => Value::from(*value),
        Node::UInt(value) => Value::from(*value),
        Node::Float(_) => Value::from(0),
        Node::Str(value) => Value::String(value.clone()),
        Node::Seq(items) => Value::Array(items.iter().map(|item| to_json(&item.value)).collect()),
        Node::Map(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, (_, value))| (key.clone(), to_json(&value.value)))
                .collect::<Map<_, _>>(),
        ),
    }
}

/// Walks `error` down to its leaves and records one error per leaf.
fn collect(error: &ValidationError<'_, '_>, root: &SpannedNode, file: &Path, errors: &mut Vec<InputError>) {
    if !error.causes.is_empty() {
        for cause in &error.causes {
            collect(cause, root, file, errors);
        }
        return;
    }
    let mut node = root;
    let mut keys = Vec::new();
    for token in &error.instance_location.tokens {
        let child = match (&node.value, token) {
            (Node::Map(entries), InstanceToken::Prop(key)) => {
                keys.push(key.as_ref());
                entries.get(key.as_ref()).map(|(_, value)| value)
            }
            _ => None,
        };
        node = child.expect("instance location points into a mapping");
    }
    let mut push = |location: serde_saphyr::Location, path: String, reason: String| {
        errors.push(InputError {
            from: Source::File {
                path: file.to_path_buf(),
                line: location.line(),
                column: location.column(),
            },
            path,
            reason,
        });
    };
    match &error.kind {
        ErrorKind::AdditionalProperties { got } => {
            let Node::Map(entries) = &node.value else {
                unreachable!("additionalProperties applies to mappings");
            };
            let reason = format!("unknown field; expected one of {}", expected_keys(&keys).join(", "));
            for key in got {
                let (location, _) = entries[key.as_ref()];
                let child = keys.iter().copied().chain([key.as_ref()]).collect::<Vec<_>>().join(".");
                push(location, child, reason.clone());
            }
        }
        kind => {
            let path = if keys.is_empty() {
                "<root>".to_owned()
            } else {
                keys.join(".")
            };
            push(node.referenced, path, reason(kind, &node.value));
        }
    }
}

/// Renders a value-free reason for a violation of `kind` by `value`.
fn reason(kind: &ErrorKind<'_, '_>, value: &Node) -> String {
    match kind {
        ErrorKind::Type { want, .. } => format!("expected {}, found {}", expected_types(*want), value.kind()),
        kind => {
            let keyword = kind
                .keyword_path()
                .map_or_else(|| "schema".to_owned(), |path| path.keyword.to_owned());
            format!("does not match the config schema ({keyword})")
        }
    }
}

/// Names the expected YAML types for a type error, leaving out `null`.
fn expected_types(want: Types) -> String {
    let names: Vec<&str> = want
        .iter()
        .filter_map(|t| match t {
            Type::Null => None,
            Type::Boolean => Some("boolean"),
            Type::Number => Some("number"),
            Type::Integer => Some("integer"),
            Type::String => Some("string"),
            Type::Array => Some("sequence"),
            Type::Object => Some("mapping"),
        })
        .collect();
    names.join(" or ")
}

/// Lists the keys accepted inside the mapping at `keys`, in setting order.
fn expected_keys(keys: &[&str]) -> Vec<&'static str> {
    let mut seen = HashSet::new();
    SETTINGS
        .iter()
        .filter_map(|setting| {
            let mut segments = setting.path.split('.');
            keys.iter()
                .all(|key| segments.next() == Some(key))
                .then(|| segments.next())?
        })
        .filter(|key| seen.insert(*key))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::env::Kind;
    use crate::node;

    pub(crate) fn document() -> Value {
        serde_json::from_str(SCHEMA).expect("valid JSON")
    }

    /// Returns the property schema for a dotted `path`.
    pub(crate) fn property<'a>(document: &'a Value, path: &str) -> Option<&'a Value> {
        path.split('.')
            .try_fold(document, |schema, key| schema.get("properties")?.get(key))
    }

    fn types(schema: &Value) -> Vec<&str> {
        schema["type"]
            .as_array()
            .expect("type is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect()
    }

    /// Collects the dotted path of every leaf property.
    fn leaves(schema: &Value, prefix: &str, out: &mut Vec<String>) {
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            out.push(prefix.to_owned());
            return;
        };
        for (key, child) in properties {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            leaves(child, &path, out);
        }
    }

    /// The schema `enum` for `choices`: every name, then `null` for unset.
    fn nullable(choices: &[&str]) -> Value {
        choices
            .iter()
            .map(|name| Value::from(*name))
            .chain([Value::Null])
            .collect()
    }

    #[test]
    fn schema_matches_the_setting_table() {
        let document = document();
        let mut paths = Vec::new();
        leaves(&document, "", &mut paths);
        let settings: Vec<&str> = SETTINGS.iter().map(|s| s.path).collect();
        assert_eq!(paths.len(), settings.len(), "schema leaves: {paths:?}");
        for setting in SETTINGS {
            let schema = property(&document, setting.path).unwrap_or_else(|| panic!("{} missing", setting.path));
            let path = setting.path;
            match setting.kind {
                Kind::Text | Kind::Path { .. } => assert_eq!(types(schema), ["string", "null"], "{path}"),
                Kind::Bool => assert_eq!(types(schema), ["boolean", "string", "null"], "{path}"),
                Kind::Int { min, max } => {
                    assert_eq!(types(schema), ["integer", "string", "null"], "{path}");
                    assert_eq!(schema["minimum"], Value::from(min), "{path}");
                    let maximum = (max != u64::MAX).then(|| Value::from(max));
                    assert_eq!(schema.get("maximum"), maximum.as_ref(), "{path}");
                }
                Kind::Enum { choices } => {
                    assert_eq!(types(schema), ["string", "null"], "{path}");
                    assert_eq!(schema["enum"], nullable(choices), "{path}");
                }
                Kind::List { choices } => {
                    assert_eq!(types(schema), ["string", "array", "null"], "{path}");
                    assert_eq!(schema["items"]["type"], "string", "{path}");
                    let names = choices.map(|choices| choices.iter().map(|name| Value::from(*name)).collect());
                    assert_eq!(schema["items"].get("enum"), names.as_ref(), "{path}");
                }
            }
        }
    }

    #[test]
    fn sections_are_strict_and_nullable() {
        let document = document();
        for section in
            std::iter::once(&document).chain(["database", "http", "pii", "ssh"].map(|key| &document["properties"][key]))
        {
            assert_eq!(section["additionalProperties"], false);
            assert_eq!(types(section), ["object", "null"]);
        }
    }

    #[test]
    fn schema_id_is_the_published_url() {
        assert_eq!(document()["$id"], URL);
    }

    /// Collects every keyword used by the schema at any depth.
    fn keywords(schema: &Value, out: &mut HashSet<String>) {
        let Some(object) = schema.as_object() else {
            return;
        };
        for (keyword, value) in object {
            out.insert(keyword.clone());
            match keyword.as_str() {
                "properties" => {
                    for child in value.as_object().expect("properties is an object").values() {
                        keywords(child, out);
                    }
                }
                "items" => keywords(value, out),
                _ => {}
            }
        }
    }

    #[test]
    fn stripped_schema_keeps_only_keywords_with_a_rendered_reason() {
        // `type` and `additionalProperties` have their own reason in
        // `reason` / `collect`; the rest are annotations and applicators.
        const STRUCTURAL: &[&str] = &[
            "type",
            "additionalProperties",
            "properties",
            "$schema",
            "$id",
            "title",
            "description",
            "default",
        ];
        let mut document = document();
        strip_values(&mut document);
        let mut used = HashSet::new();
        keywords(&document, &mut used);
        let unmapped: Vec<&String> = used.iter().filter(|k| !STRUCTURAL.contains(&k.as_str())).collect();
        assert!(unmapped.is_empty(), "add a reason for {unmapped:?}");
    }

    #[test]
    fn defaults_validate_against_the_full_schema() {
        let document = document();
        let mut compiler = Compiler::new();
        compiler.add_resource(URL, document.clone()).expect("absolute");
        let mut schemas = Schemas::new();
        let index = compiler.compile(URL, &mut schemas).expect("compiles");
        for setting in SETTINGS {
            let Some(default) = property(&document, setting.path).and_then(|s| s.get("default")) else {
                continue;
            };
            // Nest the default under its keys and validate it as a whole file.
            let instance = setting
                .path
                .rsplit('.')
                .fold(default.clone(), |value, key| serde_json::json!({ key: value }));
            assert!(schemas.validate(&instance, index).is_ok(), "{}", setting.path);
        }
    }

    fn check(yaml: &str) -> Vec<String> {
        let root = node::parse(yaml).expect("valid yaml");
        let mut errors: Vec<String> = validate(&root, Path::new("f.yaml"))
            .iter()
            .map(ToString::to_string)
            .collect();
        errors.sort();
        errors
    }

    #[test]
    fn unknown_keys_point_at_the_key() {
        assert_eq!(
            check("database:\n  hots: s3cret\nextra: 1\n"),
            [
                "f.yaml:2:3: database.hots: unknown field; expected one of backend, host, port, user, password, \
                 name, charset, ssl, ssl_ca, ssl_cert, ssl_key, ssl_verify_cert, read_only, max_pool_size, \
                 connection_timeout, query_timeout, page_size",
                "f.yaml:3:1: extra: unknown field; expected one of transport, log_level, database, http, pii, ssh",
            ]
        );
    }

    #[test]
    fn root_and_sections_must_be_mappings_or_null() {
        assert_eq!(check("- a\n"), ["f.yaml:1:1: <root>: expected mapping, found sequence"]);
        assert_eq!(
            check("pii: []\nssh: 1.5\n"),
            [
                "f.yaml:1:6: pii: expected mapping, found sequence",
                "f.yaml:2:6: ssh: expected mapping, found float",
            ]
        );
        assert!(check("").is_empty());
        assert!(check("transport:\ndatabase:\nssh: ~\n").is_empty());
    }

    #[test]
    fn setting_values_are_not_checked() {
        assert!(check("transport: 1\ndatabase:\n  port: -5\n  host: [a]\n").is_empty());
    }
}
